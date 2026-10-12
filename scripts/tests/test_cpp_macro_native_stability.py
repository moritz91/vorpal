import queue
import gc
import weakref
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from cpp_macro_native_stability import canary_disposition,Windows,Guard,Overlapped,OplockInput,OplockOutput
from cpp_native_stability_control import read_phase


class NativeStabilityTests(unittest.TestCase):
    def test_guard_rejection_cannot_be_replaced_with_equal_snapshot(self):
        row={'acted':False,'mutationSucceeded':False,'mutationError':0}
        self.assertEqual(canary_disposition(False,row,True,False),'decline-guard-unavailable')

    def test_directory_break_is_sticky_even_after_source_bytes_restored(self):
        row={'acted':True,'mutationSucceeded':True,'mutationError':0}
        self.assertEqual(canary_disposition(True,row,True,True),'decline-directory-lease-broken')
        self.assertEqual(canary_disposition(True,row,True,False),'decline-observed-mutation')

    def test_path_replacement_and_unknown_outcomes_decline(self):
        row={'acted':True,'mutationSucceeded':False,'mutationError':32}
        self.assertEqual(canary_disposition(True,row,False,False),'decline-lexical-path-or-bytes-changed')
        row['mutationError']=999
        self.assertEqual(canary_disposition(True,row,True,False),'decline-unvalidated-actor-outcome')

    def test_positive_file_canary_never_becomes_replay_authorization(self):
        row={'acted':True,'mutationSucceeded':False,'mutationError':32}
        self.assertEqual(canary_disposition(True,row,True,False),'private-fixture-stable-only')

    def test_protocol_requires_real_actor_identity_and_complete_typed_fields(self):
        valid={'phase':'ready','pid':25,'writerOpen':False,'viewOpen':True,'sectionOpen':True}
        q=queue.Queue();q.put(valid);self.assertEqual(read_phase(q,'ready'),valid)
        for value in [dict(valid,pid=True),dict(valid,pid=0),dict(valid,viewOpen='true'),
                      dict(valid,unexpected=True),{'phase':'ready','pid':25},dict(valid,phase='restored')]:
            q=queue.Queue();q.put(value)
            with self.assertRaises(ValueError):read_phase(q,'ready')

    def test_native_case_errors_and_restore_flags_are_not_inferred(self):
        for value,wanted in [({'phase':'observed','acted':True,'mutationSucceeded':False,'mutationError':True,'consumerSawChanged':False},'observed'),
                             ({'phase':'restored','originalBytesRestored':1},'restored')]:
            q=queue.Queue();q.put(value)
            with self.assertRaises(ValueError):read_phase(q,wanted)
        if sys.platform!='win32':
            with self.assertRaises(ValueError):Windows()

    def test_pending_request_buffers_survive_failed_cancellation(self):
        class Api:
            completed=False
            def WaitForSingleObject(self,event,timeout):return 0 if self.completed else 258
            def CancelIoEx(self,*args):return True
            def GetOverlappedResult(self,*args):return True
            def CloseHandle(self,*args):return True
        class Win:
            api=Api()
        win=Win();guard=Guard(win,'share-directory',[])
        guard.directory_handle=10;guard.event=11
        guard.ov=Overlapped();guard.request=OplockInput();guard.result=OplockOutput()
        guard.lease_granted=True
        reference=weakref.ref(guard)
        with self.assertRaises(RuntimeError):guard.close()
        del guard;gc.collect()
        self.assertIsNotNone(reference(),'unfinished Windows I/O lost its buffer owner')
        win.api.completed=True
        reference().close();gc.collect()
        self.assertIsNone(reference(),'completed audit request owner was not released')
