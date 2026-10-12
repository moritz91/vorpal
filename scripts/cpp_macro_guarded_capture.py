#!/usr/bin/env python3
"""Report-only guarded native captures; never a proof/cache/publication API."""
import argparse
from dataclasses import dataclass,field
import hashlib
import json
import ntpath
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys

from cpp_macro_native_dependency_audit import candidates, identity, snapshot, run, validate_plan, parse_trace
from cpp_macro_native_lifecycle import decode_records
from cpp_macro_native_stability import Guard, Windows
from cpp_native_stability_control import semantic_index

MAX_PATHS=8192
BOUNDARIES=(
    'ordinary user-mode probes do not certify complete process/native input coverage',
    'loader/mapped consumption and registry/device/network inputs are not closed',
    'explicit tool paths do not close Python/compiler/SDK/resource dependencies',
    'indexed file parents do not certify every empty/ignored source-walk directory',
    'advisory directory events do not provide an atomic publication transaction',
    'isolated MCP responses are private observations, not production-gated responses',
    'held native/MCP accesses are not independently retraced against the discovery inventory',
)


def capture_snapshot(paths):
    # The installed CLI image exceeds the probe's default 64 MiB file limit.
    # Keep an explicit controller-only 256 MiB/file, 512 MiB aggregate budget.
    return snapshot(paths,file_limit=256*1024*1024)


def path_key(path):
    value=str(path)
    if not ntpath.isabs(value) or not ntpath.splitdrive(value)[0] or '\0' in value:
        raise ValueError('absolute Windows input path required')
    if value.startswith('\\\\?\\UNC\\'):value='\\\\'+value[8:]
    elif value.startswith('\\\\?\\'):value=value[4:]
    return ntpath.normcase(ntpath.normpath(value))


def manifest_paths(data):
    """Read only the versioned index file inventory; never trust its stats."""
    if len(data)>32*1024*1024 or len(data)<24 or data[:4]!=b'VMAN':
        raise ValueError('invalid index file inventory')
    version,stamp,count=struct.unpack_from('<IQQ',data,4)
    if version!=1 or not stamp or not 1<=count<=MAX_PATHS:
        raise ValueError('unknown/unbounded index file inventory')
    offset=24;result=[];seen=set()
    for _ in range(count):
        if offset+4>len(data):raise ValueError('truncated index inventory')
        length=struct.unpack_from('<I',data,offset)[0];offset+=4
        if not 1<=length<=32768 or offset+length+16>len(data):
            raise ValueError('invalid index inventory path')
        path=data[offset:offset+length].decode('utf-8',errors='strict');offset+=length+16
        key=path_key(path)
        if key in seen:raise ValueError('duplicate physical spelling in index inventory')
        seen.add(key);result.append(path)
    if offset!=len(data):raise ValueError('trailing index inventory bytes')
    return result



def discover_context(entry,compiler,launcher,dll,directory,number,timeout):
    """Collect framed complete traces as partial observations, preserving job failure."""
    artifacts=directory/f'discovery-{number:02}-artifacts';artifacts.mkdir()
    prefix=artifacts/'native'
    error=None
    command=[str(launcher),str(dll),str(timeout*1000),'--',compiler,
             *validate_plan(entry['args'],entry.get('translationUnit',entry['source']))]
    try:
        run(command,entry['cwd'],dict(os.environ,VORPAL_NATIVE_AUDIT_TRACE=str(prefix)),
            artifacts,'native',timeout)
    except ValueError as failure:error=str(failure)
    job_path=Path(str(prefix)+'.job.bin');life_path=Path(str(prefix)+'.life.bin')
    if not job_path.is_file() or not life_path.is_file():raise ValueError('native diagnostic envelopes missing')
    job_bytes=job_path.read_bytes()
    if len(job_bytes)!=52:raise ValueError('invalid diagnostic job envelope')
    job=struct.unpack('<7I3Q',job_bytes)
    if job[0]!=12 or job[2] or not 1<=job[3]<=128 or not job[4] or job[5:]!=(0,0,0,0,0):raise ValueError('invalid diagnostic job metadata')
    lifecycle=decode_records(life_path.read_bytes())
    records=[];incomplete=[];complete=[]
    for trace in artifacts.glob('native.*.bin'):
        suffix=trace.name[len('native.'):-4]
        if suffix in ('job','life'):continue
        if not suffix.isdecimal():raise ValueError('foreign diagnostic trace')
        try:
            values=parse_trace(trace.read_bytes());records.extend(values);complete.append(int(suffix))
        except ValueError as failure:incomplete.append({'pid':int(suffix),'error':str(failure)})
    output=artifacts/'native.i'
    processes=[{k:r[k] for k in ('pid','created','image','flags')} for r in lifecycle if r['kind']=='jobAdd']
    declined=bool(error or job[1] or incomplete or job[4] not in complete)
    paths=list(dict.fromkeys([entry['source'],entry.get('translationUnit',entry['source']),compiler,
                            *candidates(records)]))
    result={'version':1,'scope':'bounded-partial-native-discovery','replayEligible':False,
        'source':entry['source'],'translationUnit':entry.get('translationUnit',entry['source']),
        'snapshots':snapshot(paths),'nativeOutputSha256':hashlib.sha256(output.read_bytes()).hexdigest(),
        'nativeOutputEqual':None,'rootTraceComplete':job[4] in complete,
        'jobStatus':job[1],'jobProcesses':job[3],'processes':processes,
        'completeTracePids':complete,'incompleteTraces':incomplete,'discoveryDeclined':declined,
        'discoveryError':error,'processCoverage':{'allJobProcessesTraced':False,
            'lifecycleReconciled':False,'jobProcesses':job[3],'completeTraces':len(complete)},
        'unresolvedProbeCount':sum(bool(r['flags'] & 1) for r in records),
        'inputCompleteness':False}
    result['auditIdentity']=identity(result)
    (directory/f'discovery-{number:02}.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    return result


def assemble_inventory(reports,explicit,indexed_paths):
    paths={};contexts=[];coverage=[]
    def add(path):
        paths.setdefault(path_key(path),str(path))
        if len(paths)>MAX_PATHS:raise ValueError('guard inventory path bound exceeded')
    for path in [*explicit,*indexed_paths]:add(path)
    for report in reports:
        if report.get('version')!=1 or report.get('replayEligible') is not False:
            raise ValueError('foreign/non-report-only native discovery')
        for path in report['snapshots']:add(path)
        contexts.append({'source':report['source'],'translationUnit':report['translationUnit'],
            'auditIdentity':report['auditIdentity'],'nativeOutputSha256':report['nativeOutputSha256']})
        coverage.append({'source':report['source'],'processCoverage':report['processCoverage'],
                         'unresolvedProbeCount':report['unresolvedProbeCount']})
    # Protect each known file name, missing search candidate and observed directory.
    # Ancestors remain snapshotted but are not advertised as a closed redirect namespace.
    values=capture_snapshot(list(paths.values()))
    for path,value in list(values.items()):
        parent=ntpath.dirname(path)
        while not Path(parent).is_dir() and ntpath.dirname(parent)!=parent:parent=ntpath.dirname(parent)
        add(parent)
    values=capture_snapshot(list(paths.values()))
    files=[p for p,v in values.items() if v['state']=='file']
    directories=[p for p,v in values.items() if v['state']=='directory']
    missing=[p for p,v in values.items() if v['state']=='missing']
    unknown=[{'path':p,'state':v['state']} for p,v in values.items()
             if v['state'] not in ('file','directory','missing')]
    reparses=[p for p,v in values.items() if any(len(a)>3 and a[3] for a in v['ancestry'])]
    return {'schema':1,'scope':'observed-native-and-indexed-file-inventory','snapshots':values,
        'files':files,'directories':directories,'missingCandidates':missing,'unknown':unknown,
        'reparsePaths':reparses,'contexts':contexts,'ordinaryCoverage':coverage,
        'discoveryDeclines':[r['source'] for r in reports if r.get('discoveryDeclined')],
        'indexedFiles':len(indexed_paths),'inventoryIdentity':identity(values),
        'boundaries':list(BOUNDARIES),'replayEligible':False,'inputCompleteness':False}


@dataclass
class StagingGate:
    """Sticky private controller barrier; never promises atomic publication."""
    check:object
    declined:bool=False
    phases:list=field(default_factory=list)

    def checkpoint(self,phase):
        problems=self.check()
        if problems:self.declined=True
        self.phases.append({'phase':phase,'problems':problems,'declined':self.declined})
        return not self.declined

    def stage(self,write_private_candidate,between_check_and_stage=None):
        if not self.checkpoint('before-private-staging'):return False
        if between_check_and_stage:between_check_and_stage()
        write_private_candidate()
        return self.checkpoint('after-private-staging')

    def report(self):
        return {'observedWindowStable':not self.declined,'phases':self.phases,
            'publicationAuthorized':False,'atomicPublicationRaceClosed':False,'replayEligible':False}


class InventoryGuard:
    def __init__(self,win,inventory):
        self.win,self.inventory=win,inventory;self.leases=[];self.pins=None
        self.errors=[];self.acquired=False

    def acquire(self):
        for directory in self.inventory['directories']:
            lease=Guard(self.win,'share-directory',[],directory);self.leases.append(lease)
            if not lease.acquire():
                self.errors.append({'directory':directory,'nativeErrors':lease.errors});return False
        self.pins=Guard(self.win,'share',self.inventory['files'])
        if not self.pins.acquire():
            self.errors.append({'fileIndex':len(self.pins.handles),'nativeErrors':self.pins.errors});return False
        self.acquired=True;return True

    def problems(self):
        if not self.acquired:return [{'reason':'guard-unavailable','errors':self.errors}]
        broken=[str(lease.directory) for lease in self.leases if lease.broken()]
        current=capture_snapshot(list(self.inventory['snapshots']))
        changed=[p for p,v in self.inventory['snapshots'].items() if current[p]!=v]
        problems=[]
        if broken:problems.append({'reason':'directory-lease-broken','directories':broken})
        if changed:problems.append({'reason':'observed-input-snapshot-changed','paths':changed})
        if self.inventory.get('discoveryDeclines'):problems.append({'reason':'declined-native-discovery','sources':self.inventory['discoveryDeclines']})
        if self.inventory['unknown']:problems.append({'reason':'unknown-input-states','inputs':self.inventory['unknown']})
        if self.inventory['reparsePaths']:problems.append({'reason':'unprotected-reparse-namespace','paths':self.inventory['reparsePaths']})
        return problems

    def close(self):
        errors=[]
        for guard in [*([self.pins] if self.pins else []),*reversed(self.leases)]:
            try:guard.close()
            except Exception as error:errors.append(error)
        if errors:raise errors[0]


def benchmark(binary,source,config,directory,label,symbol,timeout):
    report=directory/(label+'.json');index=directory/(label+'-index')
    command=[sys.executable,str(Path(__file__).with_name('benchmark_cpp_native_mcp.py')),
        '--binary',str(binary),'--source',str(source),'--config',str(config),'--index',str(index),
        '--report',str(report),'--symbol',symbol,'--build','--rounds','1','--timeout',str(timeout)]
    with (directory/(label+'.driver.log')).open('xb') as log:
        result=subprocess.run(command,stdin=subprocess.DEVNULL,stdout=log,stderr=log,timeout=timeout*5)
    if result.returncode:raise ValueError('fresh native graph capture failed; inspect private driver log')
    return index,json.loads(report.read_bytes())


def explicit_inputs(config,path,binary,launcher,dll,plan,compiler,entries):
    files=[str(p) for p in (path,binary,launcher,dll,plan,compiler,Path(__file__).resolve())]
    directories=[]
    def visit(value):
        if isinstance(value,dict):
            for item in value.values():visit(item)
        elif isinstance(value,list):
            for item in value:visit(item)
        elif isinstance(value,str) and ntpath.isabs(value) and ntpath.splitdrive(value)[0]:
            (directories if Path(value).is_dir() else files).append(value)
    visit(config)
    for entry in entries:
        files.extend([entry['source'],entry.get('translationUnit',entry['source'])])
        directories.append(entry['cwd'])
        for option in entry['args']:
            for prefix in ('-external:I','/external:I','-I','/I'):
                if option.startswith(prefix):directories.append(option[len(prefix):]);break
            if option.startswith(('/FI','-FI')):files.append(option[3:])
    # Known audit/provider siblings are explicit inputs, still not a tool closure.
    files.extend(str(p) for p in Path(__file__).parent.glob('cpp_macro_*.py'))
    provider=Path(config['cppMacroCompiler']['arguments'][0])
    files.extend(str(p) for p in provider.parent.glob('cpp_macro_*.py'))
    return list(dict.fromkeys([*files,*directories]))


def main():
    parser=argparse.ArgumentParser(__doc__)
    for name in ('binary','source','config','launcher','dll','directory'):
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--symbol',default='ReadQuatAccessor')
    parser.add_argument('--timeout',type=int,default=120)
    parser.add_argument('--discovery-timeout',type=int,default=10)
    args=parser.parse_args()
    win=Windows()
    source=args.source.resolve();root=args.directory.resolve()
    if root==source or source in root.parents or root.exists():parser.error('new external audit directory required')
    if not 1<=args.timeout<=300 or not 1<=args.discovery_timeout<=60:
        parser.error('capture timeout 1..300 and discovery timeout 1..60 seconds required')
    config=json.loads(args.config.read_bytes());provider=config['cppMacroCompiler']
    arguments=provider['arguments'];plan=Path(arguments[arguments.index('--plan')+1])
    entries=json.loads(plan.read_bytes())
    wanted={path_key(p) for p in provider['translationUnits']}
    selected=[r for r in entries if path_key(r['source']) in wanted]
    if len(selected)!=len(wanted) or len({path_key(r['source']) for r in selected})!=len(wanted):
        raise ValueError('native configured sources need unique explicit plan entries')
    if not 1<=len(selected)<=64:raise ValueError('context count exceeds bound')
    if os.environ.get('CL') or os.environ.get('_CL_'):
        raise ValueError('implicit CL/_CL_ options are not permitted')
    for entry in selected:validate_plan(entry['args'],entry.get('translationUnit',entry['source']))
    compiler=shutil.which('cl.exe')
    if not compiler:raise ValueError('load the native VS developer environment')
    root.mkdir(parents=True)
    environment=identity(dict(os.environ));config_digest=hashlib.sha256(args.config.read_bytes()).hexdigest()
    plan_digest=hashlib.sha256(plan.read_bytes()).hexdigest()
    script_digest=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    print('fresh baseline',flush=True)
    baseline_index,baseline=benchmark(args.binary,source,args.config,root,'fresh-before',args.symbol,args.timeout)
    generation=baseline_index/(baseline_index/'CURRENT').read_text(encoding='utf-8').strip()
    indexed=manifest_paths((generation/'manifest.bin').read_bytes())
    if any(ntpath.commonpath([path_key(source),path_key(p)])!=path_key(source) for p in indexed):
        raise ValueError('index inventory escapes source root')
    reports=[]
    for number,entry in enumerate(selected):
        print('native discovery',number+1,'/',len(selected),flush=True)
        reports.append(discover_context(entry,compiler,args.launcher,args.dll,root,number,args.discovery_timeout))
    explicit=explicit_inputs(config,args.config,args.binary,args.launcher,args.dll,plan,compiler,selected)
    inventory=assemble_inventory(reports,explicit,indexed)
    (root/'inventory.json').write_text(json.dumps(inventory,indent=2)+'\n',encoding='utf-8')
    print('inventory',len(inventory['files']),'files',len(inventory['directories']),'directories',flush=True)
    guard=InventoryGuard(win,inventory);gate=StagingGate(guard.problems)
    result={'schema':1,'scope':'isolated-genuine-native-capture-controller',
        'inventoryIdentity':inventory['inventoryIdentity'],'contexts':len(selected),'indexedFiles':len(indexed),
        'files':len(inventory['files']),'directories':len(inventory['directories']),
        'missingCandidates':len(inventory['missingCandidates']),'unknownInputs':inventory['unknown'],
        'discoveryDeclines':inventory['discoveryDeclines'],
        'environmentIdentity':environment,'configSha256':config_digest,'planSha256':plan_digest,
        'controllerSha256':script_digest,
        'boundaries':list(BOUNDARIES),'ordinaryInputCompleteness':False,'replayEligible':False}
    guarded=None
    try:
        result['guardAcquired']=guard.acquire();gate.checkpoint('before-native-capture')
        if result['guardAcquired']:
            print('held-guard diagnostic capture (no publication authorization)',flush=True)
            index,guarded=benchmark(args.binary,source,args.config,root,'held-guard',args.symbol,args.timeout)
            gate.checkpoint('after-native-capture-and-private-mcp-responses')
            same=semantic_index(baseline_index)==semantic_index(index)
            payload=lambda r:{row['tool']:row['contentSha256'] for row in r['requests']}
            result['freshStoredGraphAndSpansEqual']=same
            result['healthGraphSnippetPayloadsEqual']=payload(baseline)==payload(guarded)
            gate.checkpoint('after-graph-comparison')
            if not same or not result['healthGraphSnippetPayloadsEqual']:
                gate.declined=True
                gate.phases.append({'phase':'comparison','problems':[{'reason':'fresh-graph-or-payload-mismatch'}],'declined':True})
            staged=root/'private-candidate.json'
            result['candidateStaged']=gate.stage(lambda:staged.write_text(json.dumps({
                'graphSignature':identity(semantic_index(index)),'publicationAuthorized':False,'replayEligible':False}),encoding='utf-8'))
        else:result['candidateStaged']=False
        gate.checkpoint('before-guard-release')
        result['guardWindow']=gate.report();result['guardErrors']=guard.errors
    finally:guard.close()
    print('fresh post-guard capture',flush=True)
    after_index,after=benchmark(args.binary,source,args.config,root,'fresh-after',args.symbol,args.timeout)
    payload=lambda r:{row['tool']:row['contentSha256'] for row in r['requests']}
    result['semanticFilesCompared']=len(semantic_index(baseline_index))
    result['freshBeforeAfterGraphAndSpansEqual']=semantic_index(baseline_index)==semantic_index(after_index)
    result['freshBeforeAfterPayloadsEqual']=payload(baseline)==payload(after)
    result['explicitConfigPlanEnvironmentUnchanged']=(environment==identity(dict(os.environ)) and
        config_digest==hashlib.sha256(args.config.read_bytes()).hexdigest() and
        plan_digest==hashlib.sha256(plan.read_bytes()).hexdigest())
    result['controllerScriptUnchanged']=script_digest==hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    result['binaryDigestsUnchanged']=baseline['binarySha256']==after['binarySha256'] and (guarded is None or guarded['binarySha256']==baseline['binarySha256'])
    if not all(result[k] for k in ('freshBeforeAfterGraphAndSpansEqual','freshBeforeAfterPayloadsEqual','explicitConfigPlanEnvironmentUnchanged','controllerScriptUnchanged','binaryDigestsUnchanged')):
        raise ValueError('genuine before/after comparison changed; private report cannot imply stability')
    (root/'report.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({k:result[k] for k in ('contexts','indexedFiles','files','directories','guardAcquired','candidateStaged','replayEligible')}))


if __name__=='__main__':main()
