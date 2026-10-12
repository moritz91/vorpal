import hashlib
from pathlib import Path
import struct
import sys
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cpp_macro_native_input_bytes import MAGIC, RECORD, parse_input_bytes, capture_inputs


def row(kind, sequence, pid=5, status=0, flags=0, path='', payload=b'', trace=0, created=500, file_id=b'I'*16):
    name = path.encode('utf-16-le')
    return RECORD.pack(kind,status,flags,pid,1,len(name)//2,len(payload),0,
                       sequence,trace,10,11,42,123,created,0,file_id)+name+payload


def ledger(*middle):
    return MAGIC + row(1,1) + b''.join(middle) + row(5,len(middle)+2)


class NativeInputByteTests(unittest.TestCase):
    def test_completed_return_bytes_are_hashed_with_physical_identity_and_call(self):
        rows = parse_input_bytes(ledger(row(2,2,path='C:\\input.h',payload=b'actual return',trace=7)))
        self.assertEqual(rows[1]['sha256'],hashlib.sha256(b'actual return').hexdigest())
        self.assertEqual(rows[1]['fileId'],(b'I'*16).hex())
        self.assertEqual(rows[1]['traceSequence'],7)
        self.assertEqual(rows[1]['bytes'],13)

    def test_pending_read_is_explicit_and_never_contains_unfinished_buffer(self):
        rows = parse_input_bytes(ledger(row(2,2,status=0x103,flags=4|64)))
        self.assertIn('pending',rows[1]['flags'])
        self.assertFalse(rows[1]['bytes'])
        for status, flags, data in [(0x103,0,b''),(0x103,4|64,b'not completed'),(0xc0000001,64,b'failure')]:
            with self.assertRaises(ValueError):
                parse_input_bytes(ledger(row(2,2,status=status,flags=flags,payload=data)))

    def test_mapping_snapshot_cannot_be_labeled_as_actual_read_pages(self):
        value = parse_input_bytes(ledger(row(4,2,flags=32|16)))[1]
        self.assertIn('mappedPagesUnobserved',value['flags'])
        for flags,data in [(0,b''),(32,b'fake pages')]:
            with self.assertRaises(ValueError):
                parse_input_bytes(ledger(row(4,2,flags=flags,payload=data)))
        image = parse_input_bytes(ledger(row(3,2,payload=b'loader file')))[1]
        self.assertEqual(image['kind'],'loaderFile')
        self.assertIn('imageBase',image)

    def test_corrupt_truncated_lost_excessive_and_duplicate_framing_declines(self):
        valid=ledger(row(2,2,payload=b'x'))
        cases=[valid[:-1],valid+b'x',valid[1:],ledger(row(2,3)),ledger(row(2,2,flags=128)),
               ledger(row(99,2)),ledger(row(2,2,path='bad\0path')),ledger(row(1,2))]
        for data in cases:
            with self.subTest(data=data[:30]),self.assertRaises(ValueError):
                parse_input_bytes(data)

    def test_buffer_to_execution_and_native_call_binding_cannot_be_forged(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix=Path(directory)/'trace'
            trace=[{'sequence':7,'kind':'read','thread':1,'status':0,'path':'C:\\input.h','begin':10,'access':64}]
            life={'processes':[{'pid':5,'created':500}], 'records':[{'kind':'debugCreate','pid':5,'image':''}]}
            read=Path(str(prefix)+'.5.bytes.bin')
            loader=Path(str(prefix)+'.loader.bin')
            read.write_bytes(ledger(row(2,2,path='C:\\input.h',payload=b'returned',trace=7)))
            loader.write_bytes(ledger(row(3,2,payload=b'image')))
            report=capture_inputs(prefix,life,{5:trace})
            self.assertFalse(report['ordinaryExecutionInputProof'])
            self.assertFalse(report['replayEligible'])
            for created,ref in [(501,7),(500,8)]:
                read.write_bytes(ledger(row(2,2,path='C:\\input.h',trace=ref,created=created)))
                with self.assertRaises(ValueError): capture_inputs(prefix,life,{5:trace})
            read.write_bytes(ledger())
            with self.assertRaises(ValueError): capture_inputs(prefix,life,{5:trace})
            read.write_bytes(ledger(row(2,2,path='C:\\input.h',trace=7)))
            loader.write_bytes(ledger())
            with self.assertRaises(ValueError): capture_inputs(prefix,life,{5:trace})
