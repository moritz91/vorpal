#!/usr/bin/env python3
"""Isolated native Windows file/mapping/namespace guard canaries. No replay."""
import argparse
import ctypes
import json
import hashlib
import sys
import os
from pathlib import Path
import queue
import subprocess
import threading
from cpp_macro_native_stability import Windows,Guard,Overlapped,FileId,OplockInput,OplockOutput,canary_disposition
from cpp_macro_native_lifecycle import read_lifecycle

SCENARIOS=('stable-read','new-write','existing-write','existing-async-write','existing-map-open',
           'existing-map-closed','new-map','hardlink-write','file-replace','directory-shadow','directory-rename','existing-section-unmapped','existing-section-readonly-view','async-read')


def read_phase(lines,wanted):
    value=lines.get(timeout=20)
    if isinstance(value,Exception):raise value
    if not isinstance(value,dict) or value.get('phase')!=wanted:raise ValueError('unexpected native guard handshake')
    fields={'ready':{'phase','pid','writerOpen','viewOpen','sectionOpen'},
            'observed':{'phase','acted','mutationSucceeded','mutationError','consumerSawChanged','readCompleted','readPending','consumerSawOriginal'},
            'restored':{'phase','originalBytesRestored'}}
    if set(value)!=fields[wanted]:raise ValueError('unknown/missing native guard handshake fields')
    if wanted=='ready' and (type(value['pid']) is not int or not 1<=value['pid']<=0xffffffff):raise ValueError('missing actor identity')
    for key in fields[wanted]-{'phase','pid','mutationError'}:
        if type(value[key]) is not bool:raise ValueError('native guard handshake flag is not boolean')
    if wanted=='observed' and (type(value['mutationError']) is not int or not 0<=value['mutationError']<=0xffffffff):
        raise ValueError('invalid native mutation error')
    return value


def semantic_index(index):
    generation=index/(index/'CURRENT').read_text(encoding='utf-8').strip()
    selected=[]
    for name in ('nodes','edges','sigs','evidence'):
        selected.extend(p for p in (generation/name).rglob('*') if p.is_file())
    selected.extend(generation/name for name in ('names.idx','source_contexts.json','dataflow.bin','graph.bin') if (generation/name).is_file())
    if not selected:raise ValueError('fresh index semantic files missing')
    return {p.relative_to(generation).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in selected}


def graph_control(root,win,binary,template_path):
    directory=root/'fresh-graph';directory.mkdir()
    source_dir=directory/'src';source_dir.mkdir()
    source=source_dir/'guarded.cc';header=source_dir/'policy.h'
    source.write_bytes(b'#include "policy.h"\nint leaf() { return 1; }\nint caller() { CHECK(leaf()) leaf(); return 0; }\n')
    header.write_bytes(b'#define CHECK(value) if (!(value)) { throw 1; }\n')
    originals={p:p.read_bytes() for p in (source,header)}
    template=json.loads(Path(template_path).read_bytes())
    compiler=dict(template['cppMacroCompiler'])
    args=list(compiler.get('arguments',[]));position=args.index('--plan')+1
    plan=directory/'plan.json'
    plan.write_text(json.dumps([{'source':str(source),'cwd':str(source_dir),'args':[
        '/nologo','/TP','/std:c++20','/Zc:preprocessor',str(source),'/Zs']}]),encoding='utf-8')
    args[position]=str(plan)
    compiler.update(arguments=args,directory=str(source_dir),translationUnits=[str(source)])
    config=directory/'project.json'
    config.write_text(json.dumps({'ruleDirs':[],'cppMacroIncludeRoots':[],'cppMacroCompiler':compiler}),encoding='utf-8')
    signatures={};reports={};guarded_lease_broken=False
    for label in ('fresh-before','guarded','fresh-after'):
        index=directory/(label+'-index');report=directory/(label+'.json')
        guard=Guard(win,'share-directory',[source,header],source_dir) if label=='guarded' else None
        try:
            if guard and not guard.acquire():raise ValueError('private source/header guard unavailable')
            command=[sys.executable,str(Path(__file__).with_name('benchmark_cpp_native_mcp.py')),
                '--binary',str(Path(binary).resolve()),'--source',str(source_dir),'--config',str(config),
                '--index',str(index),'--report',str(report),'--symbol','caller','--build','--rounds','1','--timeout','90']
            completed=subprocess.run(command,stdin=subprocess.DEVNULL,capture_output=True,timeout=300)
            (directory/(label+'.driver.stderr')).write_bytes(completed.stderr)
            if completed.returncode:raise ValueError('fresh native graph comparison failed; inspect private artifacts')
            signatures[label]=semantic_index(index);reports[label]=json.loads(report.read_bytes())
            if guard:
                guarded_lease_broken=guard.broken()
                if guarded_lease_broken:raise ValueError('private directory lease broke during successful graph canary')
                assert guard.current_ids()==[win.snapshot(p)['identity'] for p in (source,header)]
        finally:
            if guard:guard.close()
        assert all(p.read_bytes()==data for p,data in originals.items())
    if not signatures['fresh-before']==signatures['guarded']==signatures['fresh-after']:
        raise ValueError('guarded/fresh native graph files or original spans differ')
    # MCP tool payloads cover health/diagnoses, calls and original multipart spans.
    payloads={label:{row['tool']:row['contentSha256'] for row in report['requests']}
              for label,report in reports.items()}
    if not payloads['fresh-before']==payloads['guarded']==payloads['fresh-after']:
        raise ValueError('guarded/fresh MCP responses differ')
    return {'sourceAndHeaderUnchanged':True,'freshStoredNodesEdgesContextsAndSpansEqual':True,
        'semanticFiles':len(signatures['guarded']),'healthGraphSnippetPayloadsEqual':True,
        'directoryLeaseBroken':guarded_lease_broken,'scope':'private-known-source-and-header-only',
        'nativeCapturesMandatory':True,'replayEligible':False,'ordinaryInputCompleteness':False}


def main():
    parser=argparse.ArgumentParser(__doc__)
    for name in ('launcher','actor','directory'):parser.add_argument('--'+name,required=True)
    parser.add_argument('--binary')
    parser.add_argument('--native-config')
    args=parser.parse_args()
    if bool(args.binary)!=bool(args.native_config):parser.error('binary/native-config must be supplied together')
    root=Path(args.directory).resolve();root.mkdir(parents=True,exist_ok=False)
    win=Windows()
    abi=json.loads(subprocess.check_output([args.actor,'--abi'],timeout=10))
    assert abi=={'overlapped':ctypes.sizeof(Overlapped),'fileId':ctypes.sizeof(FileId),
                 'oplockInput':ctypes.sizeof(OplockInput),'oplockOutput':ctypes.sizeof(OplockOutput),'requestOplock':0x90240}
    results={}
    for mode in ('none','range','share','share-directory'):
        for scenario in SCENARIOS:
            directory=root/(mode+'-'+scenario);directory.mkdir()
            namespace=directory/'namespace';namespace.mkdir()
            data=namespace/'input.txt';data.write_bytes(b'O'*512)
            before=win.snapshot(data);stamp=data.stat().st_mtime_ns
            prefix=directory/'job'
            env=dict(os.environ,VORPAL_NATIVE_AUDIT_TRACE=str(prefix))
            lines=queue.Queue()
            with (directory/'native.stderr').open('xb') as stderr:
                process=subprocess.Popen([args.launcher,'-','30000','--',args.actor,scenario,str(data)],
                    cwd=directory,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr)
                def reader(stream):
                    try:
                        for raw in iter(lambda:stream.readline(8193),b''):
                            if len(raw)>8192:raise ValueError('guard protocol record exceeds bound')
                            lines.put(json.loads(raw))
                    except Exception as error:lines.put(error)
                thread=threading.Thread(target=reader,args=(process.stdout,),daemon=True);thread.start()
                guard=Guard(win,mode,[data],namespace)
                try:
                    ready=read_phase(lines,'ready')
                    acquired=guard.acquire()
                    process.stdin.write(b'run\n' if acquired else b'stop\n');process.stdin.flush()
                    observed=read_phase(lines,'observed')
                    after=win.snapshot(data)
                    broken=guard.broken()
                    ids=guard.current_ids()
                    matches=before==after and (not ids or ids==[before['identity']])
                    disposition=canary_disposition(acquired,observed,matches,broken)
                    hold_through_restore=acquired and (scenario=='directory-shadow' or (mode=='range' and ('map' in scenario or scenario.startswith('existing-section-'))))
                    if not hold_through_restore:guard.close()
                    process.stdin.write(b'restore\n');process.stdin.flush()
                    restored=read_phase(lines,'restored')
                    post_restore_break=guard.broken() if hold_through_restore else broken
                    if hold_through_restore:guard.close()
                    code=process.wait(timeout=15);thread.join(timeout=1)
                    if code or not restored['originalBytesRestored']:raise ValueError('native mutation fixture failed to restore')
                    os.utime(data,ns=(stamp,stamp))
                    final=win.snapshot(data)
                    life=read_lifecycle(prefix)
                    assert ready['pid']==life['rootPid']
                    assert next(p for p in life['processes'] if p['pid']==life['rootPid'])['metadataComplete']
                    assert final==before,'private original identity/bytes not restored'
                    row={'guard':mode,'scenario':scenario,'guardAcquired':acquired,'guardErrors':guard.errors,
                         'observed':observed,'directoryLeaseBroken':broken,'physicalPathAndBytesStableDuringGuard':matches,
                         'canaryDisposition':disposition,'beforeAfterRestorationEqual':True,'guardHeldThroughRestore':hold_through_restore,
                         'directoryLeaseBrokenAfterRestore':post_restore_break,
                         'lifetimesReconciled':life['lifetimesReconciled'],'replayEligible':False,'ordinaryInputCompleteness':False}
                    (directory/'control.json').write_text(json.dumps(row,indent=2)+'\n',encoding='utf-8')
                    if mode=='none' and scenario not in ('stable-read','async-read'):assert observed['mutationSucceeded'], (mode,scenario,observed)
                    if mode=='range' and scenario in ('new-write','existing-write','existing-async-write','hardlink-write'):
                        assert acquired and not observed['mutationSucceeded'] and observed['mutationError']==33
                    if mode=='range' and scenario in ('existing-map-open','existing-map-closed','new-map','existing-section-unmapped','existing-section-readonly-view'):
                        assert observed['mutationSucceeded'] and observed['consumerSawChanged']
                    if mode.startswith('share') and scenario in ('new-write','new-map','hardlink-write','file-replace'):
                        assert acquired and not observed['mutationSucceeded'] and observed['mutationError'] in (5,32)
                    if mode.startswith('share') and scenario in ('existing-write','existing-async-write','existing-map-open','existing-map-closed','existing-section-unmapped','existing-section-readonly-view'):
                        assert not acquired and guard.errors[0]==32
                    if scenario=='async-read':assert acquired and observed['readCompleted'] and observed['consumerSawOriginal']
                    if mode=='share-directory' and scenario=='directory-shadow':
                        assert acquired and observed['mutationSucceeded'] and broken and post_restore_break and hold_through_restore and disposition.startswith('decline-')
                    if mode=='share' and scenario=='directory-shadow':
                        assert acquired and observed['mutationSucceeded'] and matches
                    results[mode+'-'+scenario]=row
                finally:
                    try:guard.close()
                    finally:
                        if process.poll() is None:process.kill();process.wait(timeout=10)
                        if process.stdin:process.stdin.close()
                        if process.stdout:process.stdout.close()
    if args.binary:results['fresh-native-graph']=graph_control(root,win,args.binary,args.native_config)
    (root/'controls.json').write_text(json.dumps(results,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({'stabilityControls':len(results),'passed':True,'replayEligible':False}))


if __name__=='__main__':main()
