#!/usr/bin/env python3
"""Native private staging/publication-gap controls; never enables replay."""
import argparse
import hashlib
import struct
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import threading
from cpp_macro_guarded_capture import InventoryGuard,StagingGate
from cpp_macro_native_dependency_audit import run,snapshot,validate_plan
from cpp_macro_native_stability import Windows
from cpp_native_stability_control import read_phase
from cpp_macro_native_lifecycle import decode_records


def diagnostic_job_status(job_bytes,life_bytes):
    """A clean root exit cannot repair outliving descendants (status 126)."""
    if len(job_bytes)!=52:raise ValueError('invalid scoped job envelope')
    job=struct.unpack('<7I3Q',job_bytes)
    if (job[0]!=12 or job[1] not in (0,126) or job[2] or not 1<=job[3]<=128
            or not job[4] or job[5:]!=(0,0,0,0,0)):
        raise ValueError('scoped native execution failed beyond diagnostic boundary')
    records=decode_records(life_bytes)
    if (not records or records[0]['kind']!='start' or records[-1]['kind']!='end'
            or records[0]['flags'] or records[-1]['flags'] or records[-1]['pid']!=job[4]
            or records[-1]['status']!=job[1] or records[-1]['parent']!=job[3]):
        raise ValueError('native lifecycle endpoints do not match scoped job')
    roots=[r for r in records if r['kind']=='root']
    exits=[r for r in records if r['kind']=='jobExit' and r['pid']==job[4]]
    if (len(roots)!=1 or roots[0]['pid']!=job[4] or roots[0]['flags']
            or roots[0]['status'] or roots[0]['parent']!=records[0]['pid']
            or not roots[0]['created'] or not roots[0]['image'] or len(exits)!=1
            or exits[0]['status'] or exits[0]['flags'] or exits[0]['created']!=roots[0]['created']):
        raise ValueError('native root did not complete successfully')
    return {'jobStatus':job[1],'jobClosureDeclined':job[1]!=0,
        'rootExitObserved':True,'completeInputProof':False,'publicationAuthorized':False}


def main():
    parser=argparse.ArgumentParser(__doc__)
    for name in ('launcher','actor','directory'):parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args();win=Windows();root=args.directory.resolve()
    root.mkdir(parents=True,exist_ok=False)
    compiler=shutil.which('cl.exe')
    if not compiler:raise ValueError('load the native VS developer environment')
    results={}
    cases=('stable','existing-writer','write-blocked','shadow-before-staging',
           'shadow-in-staging-gap','shadow-after-final-check')
    for case in cases:
        directory=root/case;directory.mkdir();namespace=directory/'source';namespace.mkdir()
        source=namespace/'probe.cc';header=namespace/'policy.h'
        source.write_bytes(b'#include "policy.h"\n#if __has_include("optional.h")\nint chosen() { return 2; }\n#else\nint chosen() { return BASE; }\n#endif\n')
        header.write_bytes(b'#define BASE 1\n')
        originals={p:p.read_bytes() for p in (source,header)}
        jobs={}
        def compile_native(label):
            prefix=directory/label
            try:
                run([str(args.launcher),'-','30000','--',compiler,
                    *validate_plan(['/nologo','/TP','/std:c++20','/Zc:preprocessor',str(source),'/Zs'],source)],namespace,
                    dict(os.environ,VORPAL_NATIVE_AUDIT_TRACE=str(prefix)),directory,label,30)
            except ValueError:
                # This control compares observations only. Retain a declined job;
                # never convert its stdout into a complete native capture/proof.
                jobs[label]=diagnostic_job_status(Path(str(prefix)+'.job.bin').read_bytes(),
                                                Path(str(prefix)+'.life.bin').read_bytes())
                if not jobs[label]['jobClosureDeclined']:raise
            else:
                jobs[label]=diagnostic_job_status(Path(str(prefix)+'.job.bin').read_bytes(),
                                                Path(str(prefix)+'.life.bin').read_bytes())
            return hashlib.sha256((directory/(label+'.i')).read_bytes()).hexdigest()
        baseline=compile_native('fresh-before')
        paths=[str(source),str(header),str(namespace)]
        inventory={'files':paths[:2],'directories':paths[2:],'snapshots':snapshot(paths),'unknown':[],'reparsePaths':[]}
        scenario=('existing-write' if case=='existing-writer' else 'new-write' if case=='write-blocked'
                  else 'directory-shadow' if case.startswith('shadow-') else 'stable-read')
        lines=queue.Queue();prefix=directory/'actor-job'
        with (directory/'actor.stderr').open('xb') as error:
            actor=subprocess.Popen([str(args.launcher),'-','30000','--',str(args.actor),scenario,str(header)],
                cwd=directory,env=dict(os.environ,VORPAL_NATIVE_AUDIT_TRACE=str(prefix)),
                stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=error)
            def reader():
                try:
                    for raw in iter(lambda:actor.stdout.readline(8193),b''):
                        if len(raw)>8192:raise ValueError('actor protocol exceeded bound')
                        lines.put(json.loads(raw))
                except Exception as problem:lines.put(problem)
            thread=threading.Thread(target=reader,daemon=True);thread.start()
            guard=InventoryGuard(win,inventory);gate=StagingGate(guard.problems);observed=None
            staged=directory/'private-candidate.json'
            def act(command=b'run\n'):
                nonlocal observed
                if observed is not None:raise ValueError('actor mutation executed twice')
                actor.stdin.write(command);actor.stdin.flush();observed=read_phase(lines,'observed')
            try:
                ready=read_phase(lines,'ready');acquired=guard.acquire()
                before=gate.checkpoint('before-native-capture')
                captured=None;late_change=False
                if acquired:
                    if case in ('stable','write-blocked'):act()
                    captured=compile_native('held-guard')
                    gate.checkpoint('after-native-capture')
                    if case=='shadow-before-staging':act()
                    staged_ok=gate.stage(lambda:staged.write_text(json.dumps({
                        'nativeOutputSha256':captured,'publicationAuthorized':False,'replayEligible':False}),encoding='utf-8'),
                        act if case=='shadow-in-staging-gap' else None)
                    if case=='shadow-after-final-check':
                        assert staged_ok
                        act();late_change=True
                    gate.checkpoint('before-guard-release')
                else:
                    act(b'stop\n');staged_ok=False
                if case=='existing-writer':assert not acquired and not before and not staged.exists()
                if case=='write-blocked':assert observed['mutationError']==32 and not observed['mutationSucceeded']
                if case.startswith('shadow-'):
                    assert observed['mutationSucceeded'] and gate.declined
                    assert compile_native('fresh-with-shadow')!=baseline
                    if case=='shadow-before-staging':assert not staged.exists()
                    if case=='shadow-in-staging-gap':assert staged.exists() and not staged_ok
                if case in ('stable','write-blocked'):assert staged_ok and not gate.declined and captured==baseline
                result={'guardAcquired':acquired,'capturedNativeOutputEqual':captured==baseline if captured else None,
                    'privateCandidateExists':staged.exists(),'stagingCheckPassedBeforeLateChange':staged_ok,
                    'mutationAfterFinalStagingCheck':late_change,'actor':observed,'guardErrors':guard.errors,
                    'scope':'private-file-guard-observations','nativeJobs':jobs,
                    'gate':gate.report(),'publicationAuthorized':False,'ordinaryInputCompleteness':False,'replayEligible':False}
                guard.close()
                actor.stdin.write(b'restore\n');actor.stdin.flush()
                restored=read_phase(lines,'restored')
                if actor.wait(timeout=15) or not restored['originalBytesRestored']:raise ValueError('private actor did not restore')
                thread.join(timeout=1)
                assert all(p.read_bytes()==data for p,data in originals.items())
                assert not (namespace/'optional.h').exists()
                assert compile_native('fresh-after')==baseline
                results[case]=result
            finally:
                try:guard.close()
                finally:
                    if actor.poll() is None:actor.kill();actor.wait(timeout=10)
                    actor.stdin.close();actor.stdout.close()
    (root/'controls.json').write_text(json.dumps(results,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({'nativeStagingControls':len(results),'passed':True,'publicationAuthorized':False,'replayEligible':False}))


if __name__=='__main__':main()
