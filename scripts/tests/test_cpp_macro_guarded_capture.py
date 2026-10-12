import json
from pathlib import Path
import struct
import tempfile
import sys
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from cpp_native_capture_control import diagnostic_job_status
from cpp_macro_native_lifecycle import MAGIC as LIFE_MAGIC,RECORD as LIFE_RECORD
from cpp_macro_guarded_capture import manifest_paths,StagingGate,assemble_inventory,InventoryGuard,path_key,capture_snapshot


class GuardedCaptureTests(unittest.TestCase):
    def inventory(self,paths):
        body=b''.join(struct.pack('<I',len(p.encode()))+p.encode()+struct.pack('<QQ',1,2) for p in paths)
        return b'VMAN'+struct.pack('<IQQ',1,99,len(paths))+body

    def test_exact_versioned_inventory_not_timestamp_authorization(self):
        paths=['C:/project/a.cc','C:/project/b.h']
        self.assertEqual(manifest_paths(self.inventory(paths)),paths)
        for bad in (self.inventory(paths)[:-1],self.inventory(paths)+b'x',b'old',
                    self.inventory(['C:/project/a.cc','c:/project/A.cc']),self.inventory(['../escape'])):
            with self.assertRaises((ValueError,UnicodeError)):manifest_paths(bad)

    def test_preexisting_conflict_prevents_staging_and_stays_declined(self):
        problems=[{'reason':'guard-unavailable'}];called=[]
        gate=StagingGate(lambda:problems)
        self.assertFalse(gate.checkpoint('capture'))
        problems=[]
        self.assertFalse(gate.stage(lambda:called.append(True)))
        self.assertFalse(called)
        self.assertFalse(gate.report()['publicationAuthorized'])

    def test_mutation_in_check_to_stage_gap_is_rejected(self):
        problems=[];staged=[]
        gate=StagingGate(lambda:list(problems))
        self.assertFalse(gate.stage(lambda:staged.append('quarantined'),
            lambda:problems.append({'reason':'directory-lease-broken'})))
        self.assertEqual(staged,['quarantined'])
        problems.clear()
        self.assertFalse(gate.checkpoint('restored'))

    def test_final_check_is_not_an_atomic_publication_contract(self):
        problems=[];gate=StagingGate(lambda:list(problems))
        self.assertTrue(gate.stage(lambda:None))
        self.assertFalse(gate.report()['publicationAuthorized'])
        self.assertFalse(gate.report()['atomicPublicationRaceClosed'])
        problems.append({'reason':'late-break'})
        self.assertFalse(gate.checkpoint('release'))

    def test_inventory_preserves_unknown_and_missing_candidates(self):
        state={'C:/project/a.cc':{'state':'file','ancestry':[]},
               'C:/project/optional.h':{'state':'missing','ancestry':[]},
               'C:/project/unreadable':{'state':'unavailable','ancestry':[]},
               'C:/project':{'state':'directory','ancestry':[]}}
        report={'version':1,'replayEligible':False,'snapshots':state,'source':'C:/project/a.cc',
                'translationUnit':'C:/project/a.cc','auditIdentity':'a','nativeOutputSha256':'b',
                'processCoverage':{'allJobProcessesTraced':False},'unresolvedProbeCount':2}
        known={path_key(p):v for p,v in state.items()}
        known[path_key('C:/')]={'state':'directory','ancestry':[]}
        with patch('cpp_macro_guarded_capture.snapshot',side_effect=lambda paths,**kwargs:{p:known[path_key(p)] for p in paths}), \
             patch.object(Path,'is_dir',return_value=True):
            value=assemble_inventory([report],[],['C:/project/a.cc'])
        self.assertEqual(value['missingCandidates'],['C:/project/optional.h'])
        self.assertEqual(value['unknown'],[{'path':'C:/project/unreadable','state':'unavailable'}])
        self.assertFalse(value['ordinaryCoverage'][0]['processCoverage']['allJobProcessesTraced'])
        self.assertFalse(value['inputCompleteness'])

    def test_cleanup_drains_all_owners_after_one_guard_failure(self):
        events=[]
        class Owner:
            def __init__(self,name,fail=False):self.name,self.fail=name,fail
            def close(self):
                events.append(self.name)
                if self.fail:raise RuntimeError('pending native cancellation')
        guard=InventoryGuard(None,{})
        guard.pins=Owner('pins',True);guard.leases=[Owner('first'),Owner('second')]
        with self.assertRaises(RuntimeError):guard.close()
        self.assertEqual(events,['pins','second','first'])

    def test_large_tool_snapshot_has_explicit_budget_without_global_relaxation(self):
        from cpp_macro_native_dependency_audit import snapshot,MAX_FILE
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'tool.bin';path.write_bytes(b'a'*32)
            self.assertEqual(snapshot([str(path)],file_limit=16)[str(path)]['state'],'over-limit')
            self.assertEqual(snapshot([str(path)],file_limit=64)[str(path)]['state'],'file')
            with self.assertRaises(ValueError):snapshot([str(path)],file_limit=1024*1024*1024)
        with patch('cpp_macro_guarded_capture.snapshot',return_value={}) as native:
            capture_snapshot(['C:/tools/large.exe'])
            self.assertEqual(native.call_args.kwargs['file_limit'],256*1024*1024)
        self.assertEqual(MAX_FILE,64*1024*1024)

    def test_extended_dos_prefix_is_a_spelling_not_redirect_canonicalization(self):
        self.assertEqual(path_key(r'\\?\C:\project\a.cc'),path_key('C:/project/a.cc'))
        self.assertNotEqual(path_key('C:/link/a.cc'),path_key('C:/real/a.cc'))

    def test_clean_compiler_exit_never_repairs_a_declined_whole_job(self):
        def life(kind,pid,status,sequence,name='',parent=1):
            encoded=name.encode('utf-16-le')
            return LIFE_RECORD.pack(kind,pid,parent,status,0,len(encoded)//2,sequence,sequence,99)+encoded
        ledger=LIFE_MAGIC+life(1,1,0,1)+life(2,5,0,2,'C:/compiler/cl.exe')+life(5,5,0,3)+life(9,5,126,4,parent=3)
        job=struct.pack('<7I3Q',12,126,0,3,5,0,0,0,0,0)
        status=diagnostic_job_status(job,ledger)
        self.assertTrue(status['jobClosureDeclined'])
        self.assertFalse(status['completeInputProof'])
        self.assertFalse(status['publicationAuthorized'])
        with self.assertRaises(ValueError):diagnostic_job_status(job,LIFE_MAGIC+life(1,1,0,1)+life(2,5,0,2,'C:/compiler/cl.exe')+life(5,5,1,3)+life(9,5,126,4,parent=3))
        with self.assertRaises(ValueError):diagnostic_job_status(struct.pack('<7I3Q',12,124,0,3,5,0,0,0,0,0),ledger)
