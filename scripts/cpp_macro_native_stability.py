"""Report-only Win32 file pins/byte locks/directory read-oplock canaries.

Never an extraction environment or production replay gate. No global tracing,
privileges, driver/service installation, or mutation of protected input files.
"""
import ctypes
from ctypes import wintypes as w
import hashlib
import os
from pathlib import Path


class Overlapped(ctypes.Structure):
    _fields_ = [('Internal',ctypes.c_size_t),('InternalHigh',ctypes.c_size_t),
                ('Offset',w.DWORD),('OffsetHigh',w.DWORD),('hEvent',w.HANDLE)]


class FileId(ctypes.Structure):
    _fields_ = [('volume',ctypes.c_uint64),('id',ctypes.c_ubyte*16)]


class OplockInput(ctypes.Structure):
    _fields_ = [('version',w.WORD),('length',w.WORD),('level',w.DWORD),('flags',w.DWORD)]


class OplockOutput(ctypes.Structure):
    _fields_ = [('version',w.WORD),('length',w.WORD),('original',w.DWORD),('new',w.DWORD),
                ('flags',w.DWORD),('access',w.DWORD),('share',w.WORD)]


class Windows:
    def __init__(self):
        if os.name!='nt': raise ValueError('native Windows required for guard tests')
        self.api=ctypes.WinDLL('kernel32',use_last_error=True)
        declarations={
            'CreateFileW':([w.LPCWSTR,w.DWORD,w.DWORD,w.LPVOID,w.DWORD,w.DWORD,w.HANDLE],w.HANDLE),
            'CloseHandle':([w.HANDLE],w.BOOL),
            'GetFileInformationByHandleEx':([w.HANDLE,ctypes.c_int,w.LPVOID,w.DWORD],w.BOOL),
            'LockFileEx':([w.HANDLE,w.DWORD,w.DWORD,w.DWORD,w.DWORD,ctypes.POINTER(Overlapped)],w.BOOL),
            'UnlockFileEx':([w.HANDLE,w.DWORD,w.DWORD,w.DWORD,ctypes.POINTER(Overlapped)],w.BOOL),
            'CreateEventW':([w.LPVOID,w.BOOL,w.BOOL,w.LPCWSTR],w.HANDLE),
            'WaitForSingleObject':([w.HANDLE,w.DWORD],w.DWORD),
            'CancelIoEx':([w.HANDLE,ctypes.POINTER(Overlapped)],w.BOOL),
            'GetOverlappedResult':([w.HANDLE,ctypes.POINTER(Overlapped),ctypes.POINTER(w.DWORD),w.BOOL],w.BOOL),
            'DeviceIoControl':([w.HANDLE,w.DWORD,w.LPVOID,w.DWORD,w.LPVOID,w.DWORD,ctypes.POINTER(w.DWORD),ctypes.POINTER(Overlapped)],w.BOOL)}
        for name,(args,result) in declarations.items():
            func=getattr(self.api,name);func.argtypes=args;func.restype=result
        self.invalid=ctypes.c_void_p(-1).value

    def open(self,path,share=7,flags=0):
        handle=self.api.CreateFileW(str(path),0x80000000,share,None,3,flags,None)
        return None if handle==self.invalid else handle

    def identity(self,handle):
        value=FileId()
        if not self.api.GetFileInformationByHandleEx(handle,18,ctypes.byref(value),ctypes.sizeof(value)):
            raise OSError(ctypes.get_last_error(),'file handle identity unavailable')
        return [value.volume,bytes(value.id).hex()]

    def snapshot(self,path):
        handle=self.open(path)
        if not handle: return {'state':'unavailable','error':ctypes.get_last_error()}
        try: ident=self.identity(handle)
        finally:self.api.CloseHandle(handle)
        data=Path(path).read_bytes()
        return {'state':'file','identity':ident,'sha256':hashlib.sha256(data).hexdigest(),'size':len(data)}


# A failed cancellation must not free ctypes buffers while the kernel may still
# reference them. This bounded audit aborts on failure; retain owners until a later
# successful drain or process exit, never retry by pretending the guard closed.
_PENDING_OPLOCKS=[]


class Guard:
    """Holds ONLY explicit private test files; never claims complete inputs."""
    def __init__(self,win,mode,paths,directory=None):
        self.win,self.mode,self.paths,self.directory=win,mode,list(paths),directory
        self.handles=[];self.ids=[];self.errors=[];self.range_ops=[]
        self.directory_handle=None;self.event=None;self.ov=None;self.request=None;self.result=None
        self.lease_granted=False;self.lease_broken=False;self.acquired=False

    def acquire(self):
        if self.mode=='none':self.acquired=True;return True
        for path in self.paths:
            handle=self.win.open(path,7 if self.mode in ('none','range') else 1)
            if not handle:
                self.errors.append(ctypes.get_last_error());return False
            self.handles.append(handle);self.ids.append(self.win.identity(handle))
            if self.mode=='range':
                ov=Overlapped()
                if not self.win.api.LockFileEx(handle,1,0,0xffffffff,0xffffffff,ctypes.byref(ov)):
                    self.errors.append(ctypes.get_last_error());return False
                self.range_ops.append((handle,ov))
        if self.mode=='share-directory':
            self.directory_handle=self.win.open(self.directory,7,0x02000000|0x40000000)
            if not self.directory_handle:
                self.errors.append(ctypes.get_last_error());return False
            self.event=self.win.api.CreateEventW(None,True,False,None)
            if not self.event:raise OSError(ctypes.get_last_error(),'oplock event unavailable')
            self.ov=Overlapped();self.ov.hEvent=self.event
            self.request=OplockInput(1,ctypes.sizeof(OplockInput),1,1)
            self.result=OplockOutput();amount=w.DWORD()
            ok=self.win.api.DeviceIoControl(self.directory_handle,0x00090240,
                ctypes.byref(self.request),ctypes.sizeof(self.request),ctypes.byref(self.result),ctypes.sizeof(self.result),
                ctypes.byref(amount),ctypes.byref(self.ov))
            error=0 if ok else ctypes.get_last_error()
            if ok or error!=997:
                self.errors.append(error);return False
            self.lease_granted=True
            if self.broken():return False
        self.acquired=True;return True

    def broken(self):
        if self.lease_granted:
            state=self.win.api.WaitForSingleObject(self.event,0)
            if state not in (0,258):raise OSError(ctypes.get_last_error(),'oplock state unavailable')
            self.lease_broken |= state==0
        return self.lease_broken

    def current_ids(self):return [self.win.identity(h) for h in self.handles]

    def close(self):
        if self.lease_granted:
            # Request buffers stay alive until Windows reports cancellation or
            # break completion. A timeout declines the harness; never free them
            # while the asynchronous kernel request could still reference them.
            try:
                self.broken()
                ok=self.win.api.CancelIoEx(self.directory_handle,ctypes.byref(self.ov))
                if not ok and ctypes.get_last_error()!=1168:
                    raise OSError(ctypes.get_last_error(),'oplock cancellation failed')
                if self.win.api.WaitForSingleObject(self.event,10000)!=0:
                    raise RuntimeError('oplock cancellation did not complete within bound')
                amount=w.DWORD()
                ok=self.win.api.GetOverlappedResult(self.directory_handle,ctypes.byref(self.ov),ctypes.byref(amount),False)
                if not ok and ctypes.get_last_error()!=995:
                    raise OSError(ctypes.get_last_error(),'oplock completion unavailable')
            except Exception:
                if self not in _PENDING_OPLOCKS:_PENDING_OPLOCKS.append(self)
                raise
            self.lease_granted=False
            if self in _PENDING_OPLOCKS:_PENDING_OPLOCKS.remove(self)
        for handle,ov in self.range_ops:
            if not self.win.api.UnlockFileEx(handle,0,0xffffffff,0xffffffff,ctypes.byref(ov)):
                raise OSError(ctypes.get_last_error(),'range unlock failed')
        self.range_ops=[]
        for handle in self.handles:self.win.api.CloseHandle(handle)
        self.handles=[]
        if self.directory_handle:self.win.api.CloseHandle(self.directory_handle);self.directory_handle=None
        if self.event:self.win.api.CloseHandle(self.event);self.event=None


def canary_disposition(acquired,observed,identity_equal,lease_broken):
    if not acquired:return 'decline-guard-unavailable'
    if lease_broken:return 'decline-directory-lease-broken'
    if not identity_equal:return 'decline-lexical-path-or-bytes-changed'
    if observed['mutationSucceeded']:return 'decline-observed-mutation'
    if not observed['acted'] or observed['mutationError'] not in (0,5,32,33):return 'decline-unvalidated-actor-outcome'
    return 'private-fixture-stable-only'
