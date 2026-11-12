//! Isolated allocation accounting for the cache retention workloads.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use middi::controls::ParameterFamily;
use middi::events::Payload;
use middi::messages::{AnyMessage, Command, ControlChange, NoteOn, ParameterSet, ProgramChange};
use middi::universe::{CacheOptions, Retention, Universe};

struct Counted;
static BYTES: AtomicIsize = AtomicIsize::new(0);
// SAFETY: Every allocation and deallocation is forwarded with the original layout.
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller provides the allocator's required valid layout.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            BYTES.fetch_add(layout.size() as isize, Ordering::Relaxed);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        BYTES.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: This pointer and layout originate from the delegated allocation.
        unsafe {
            System.dealloc(pointer, layout);
        }
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;

fn retained(retention: Retention, workload: u8) -> isize {
    let before = BYTES.load(Ordering::Relaxed);
    let mut cache = Universe::new(CacheOptions {
        default: retention,
        ..Default::default()
    });
    if workload == 1 {
        for channel in 1..=16 {
            cache
                .feed(
                    &Payload::Message(AnyMessage::ProgramChange(
                        ProgramChange::new(8)
                            .unwrap()
                            .with_channel(channel)
                            .unwrap(),
                    )),
                    Some(0),
                )
                .unwrap();
            for note in 0..128 {
                cache
                    .feed(
                        &Payload::Message(AnyMessage::NoteOn(
                            NoteOn::new(note).unwrap().with_channel(channel).unwrap(),
                        )),
                        Some(0),
                    )
                    .unwrap();
            }
        }
    } else if workload == 2 {
        for parameter in 0..1024 {
            for message in ParameterSet::new(parameter, 1285)
                .unwrap()
                .with_channel(1)
                .unwrap()
                .with_family(ParameterFamily::Nrpn)
                .unwrap()
                .expand()
                .unwrap()
            {
                cache.feed(&Payload::Message(message), Some(0)).unwrap();
            }
        }
        cache
            .feed(
                &Payload::Message(AnyMessage::ControlChange(
                    ControlChange::new(1, 1).unwrap().with_channel(1).unwrap(),
                )),
                Some(0),
            )
            .unwrap();
    }
    let used = BYTES.load(Ordering::Relaxed) - before + std::mem::size_of::<Universe>() as isize;
    drop(cache);
    used
}

#[test]
fn report_full_and_minimum_live_allocations() {
    for (workload, name) in [(0, "idle"), (1, "performance"), (2, "1024 parameters")] {
        let full = retained(Retention::Full, workload);
        let minimum = retained(Retention::Minimum, workload);
        println!("{name}: full {full} bytes, minimum {minimum} bytes");
        assert!(minimum <= full);
        if workload == 1 {
            assert!(minimum < full);
        }
    }
}
