use mach2::kern_return::KERN_SUCCESS;
use mach2::traps::mach_task_self;
use mach2::vm::{mach_vm_allocate, mach_vm_deallocate};
use mach2::vm_statistics::{VM_FLAGS_ANYWHERE, VM_FLAGS_FIXED};
use mach2::vm_types::mach_vm_address_t;
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::c_void;
use std::ptr;

pub const LARGE: usize = 1 << 20;
const GRANULE: usize = 1 << 14;
const ALIGN: usize = 1 << 12;
const RUST_TAG: i32 = 241;
const ARRAY_BUFFER_TAG: i32 = 242;

fn span(size: usize) -> usize {
    size.next_multiple_of(GRANULE)
}

fn large(size: usize, align: usize) -> bool {
    size >= LARGE && align <= ALIGN
}

fn map(len: usize, tag: i32) -> *mut u8 {
    let mut address: mach_vm_address_t = 0;
    let done = unsafe { mach_vm_allocate(mach_task_self(), &mut address, len as u64, VM_FLAGS_ANYWHERE | (tag << 24)) };
    if done == KERN_SUCCESS { address as *mut u8 } else { ptr::null_mut() }
}

fn map_at(at: *mut u8, len: usize, tag: i32) -> bool {
    let mut address = at as mach_vm_address_t;
    unsafe { mach_vm_allocate(mach_task_self(), &mut address, len as u64, VM_FLAGS_FIXED | (tag << 24)) == KERN_SUCCESS }
}

fn unmap(at: *mut u8, len: usize) {
    unsafe { mach_vm_deallocate(mach_task_self(), at as mach_vm_address_t, len as u64) };
}

unsafe fn remap(at: *mut u8, size: usize, new_size: usize, tag: i32) -> *mut u8 {
    let (old, new) = (span(size), span(new_size));
    if new <= old {
        if new < old {
            unmap(unsafe { at.add(new) }, old - new);
        }
        return at;
    }
    if map_at(unsafe { at.add(old) }, new - old, tag) {
        return at;
    }
    let moved = map(new, tag);
    if !moved.is_null() {
        unsafe { ptr::copy_nonoverlapping(at, moved, size) };
        unmap(at, old);
    }
    moved
}

pub struct Alloc;

unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if large(layout.size(), layout.align()) { map(span(layout.size()), RUST_TAG) } else { unsafe { System.alloc(layout) } }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if large(layout.size(), layout.align()) { map(span(layout.size()), RUST_TAG) } else { unsafe { System.alloc_zeroed(layout) } }
    }

    unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
        if large(layout.size(), layout.align()) { unmap(at, span(layout.size())) } else { unsafe { System.dealloc(at, layout) } }
    }

    unsafe fn realloc(&self, at: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let align = layout.align();
        match (large(layout.size(), align), large(new_size, align)) {
            (false, false) => unsafe { System.realloc(at, layout, new_size) },
            (true, true) => unsafe { remap(at, layout.size(), new_size, RUST_TAG) },
            _ => {
                let moved = unsafe { self.alloc(Layout::from_size_align_unchecked(new_size, align)) };
                if !moved.is_null() {
                    unsafe {
                        ptr::copy_nonoverlapping(at, moved, layout.size().min(new_size));
                        self.dealloc(at, layout);
                    }
                }
                moved
            }
        }
    }
}

struct ArrayBuffers;

unsafe extern "C" fn allocate(_: &ArrayBuffers, len: usize) -> *mut c_void {
    if len >= LARGE { map(span(len), ARRAY_BUFFER_TAG).cast() } else { unsafe { libc::calloc(len, 1) } }
}

unsafe extern "C" fn allocate_uninitialized(_: &ArrayBuffers, len: usize) -> *mut c_void {
    if len >= LARGE { map(span(len), ARRAY_BUFFER_TAG).cast() } else { unsafe { libc::malloc(len) } }
}

unsafe extern "C" fn free(_: &ArrayBuffers, data: *mut c_void, len: usize) {
    if len >= LARGE { unmap(data.cast(), span(len)) } else { unsafe { libc::free(data) } }
}

unsafe extern "C" fn drop(_: *const ArrayBuffers) {}

static ARRAY_BUFFERS: ArrayBuffers = ArrayBuffers;
static VTABLE: v8::RustAllocatorVtable<ArrayBuffers> = v8::RustAllocatorVtable { allocate, allocate_uninitialized, free, drop };

pub(crate) fn array_buffers() -> v8::UniqueRef<v8::Allocator> {
    unsafe { v8::new_rust_allocator(&ARRAY_BUFFERS, &VTABLE) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mach2::vm_region::{VM_REGION_EXTENDED_INFO, vm_region_extended_info};

    fn tag_at(at: *const u8) -> Option<u32> {
        let mut address = at as mach_vm_address_t;
        let mut size = 0;
        let mut info = vm_region_extended_info::default();
        let mut count = vm_region_extended_info::count();
        let mut object = 0;
        let done = unsafe {
            mach2::vm::mach_vm_region(
                mach_task_self(),
                &mut address,
                &mut size,
                VM_REGION_EXTENDED_INFO,
                (&mut info as *mut vm_region_extended_info).cast(),
                &mut count,
                &mut object,
            )
        };
        (done == KERN_SUCCESS && address <= at as mach_vm_address_t).then_some(info.user_tag)
    }

    fn zeroed(at: *const u8, len: usize) -> bool {
        unsafe { std::slice::from_raw_parts(at, len) }.iter().all(|&b| b == 0)
    }

    #[test]
    fn large_blocks_are_mapped_and_returned() {
        let layout = Layout::from_size_align(3 << 20, 8).unwrap();
        let at = unsafe { Alloc.alloc_zeroed(layout) };
        assert_eq!(at as usize % GRANULE, 0);
        assert_eq!(tag_at(at), Some(RUST_TAG as u32));
        assert!(zeroed(at, layout.size()));
        unsafe { at.add(layout.size() - 1).write(7) };
        unsafe { Alloc.dealloc(at, layout) };
        assert_ne!(tag_at(at), Some(RUST_TAG as u32));

        let small = Layout::from_size_align(1000, 8).unwrap();
        let mut at = unsafe { Alloc.alloc(small) };
        unsafe { ptr::write_bytes(at, 5, 1000) };
        let mut size = 1000;
        for next in [LARGE + 1, 5 << 20, 64 << 20, 2 << 20, LARGE, 100] {
            at = unsafe { Alloc.realloc(at, Layout::from_size_align(size, 8).unwrap(), next) };
            assert!(!at.is_null());
            let bytes = unsafe { std::slice::from_raw_parts_mut(at, next) };
            assert!(bytes[..next.min(1000)].iter().all(|&b| b == 5));
            bytes[next - 1] = if next > 1000 { 9 } else { 5 };
            size = next;
        }
        unsafe { Alloc.dealloc(at, Layout::from_size_align(size, 8).unwrap()) };

        let aligned = Layout::from_size_align(2 << 20, 1 << 16).unwrap();
        let at = unsafe { Alloc.alloc(aligned) };
        assert_eq!(at as usize % (1 << 16), 0);
        assert_ne!(tag_at(at), Some(RUST_TAG as u32));
        unsafe { Alloc.dealloc(at, aligned) };

        let at = unsafe { allocate(&ARRAY_BUFFERS, 4 << 20) };
        assert_eq!(tag_at(at.cast()), Some(ARRAY_BUFFER_TAG as u32));
        assert!(zeroed(at.cast(), 4 << 20));
        unsafe { free(&ARRAY_BUFFERS, at, 4 << 20) };
        assert_ne!(tag_at(at.cast()), Some(ARRAY_BUFFER_TAG as u32));
        let at = unsafe { allocate(&ARRAY_BUFFERS, 64) };
        assert!(zeroed(at.cast(), 64));
        unsafe { free(&ARRAY_BUFFERS, at, 64) };
    }
}
