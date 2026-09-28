use super::*;

/// Test that the scratch pool actually reuses a Vec's allocation across
/// calls once one is established. This exercises the REAL
/// `TripleVertexBuffer::map_instances` and
/// `TripleVertexBuffer::accumulate_instances` methods, past the first
/// two calls -- see `test_first_accumulate_swaps_empty_accumulator` for
/// the swap path. Call 1 (swap) displaces the buffer's original,
/// still-empty (capacity 0 for a brand new buffer) accumulator storage
/// into the pool; call 2 (extend path) pops THAT and forces its first
/// real allocation by pushing into it, so its pointer necessarily
/// changes partway through call 2 -- asserting reuse across calls 1->2
/// would be asserting an implementation detail of `Vec`'s
/// grow-from-empty behavior, not the pool. Calls 3 and 4 are where the
/// pool's own job (reusing an established allocation) is actually
/// tested: call 3 pops what call 2 returned, and call 4 must pop the
/// EXACT SAME allocation call 3 used and returned. If someone reverts
/// the pooling fix (changes `map_instances` back to `Vec::with_capacity`
/// on every call), this test would fail because call 4 would allocate a
/// fresh Vec (different pointer) instead of reusing call 3's.
#[test]
fn test_scratch_pool_reuses_capacity() {
    // TripleVertexBuffer::new accepts an empty vec for bufs.
    // Neither map_instances nor accumulate_instances touches self.bufs,
    // so this is safe for testing the pool logic without a real wgpu device.
    let tvb = TripleVertexBuffer::new(vec![], 100);

    let quad_a = crate::quad::QuadInstance {
        position: [10.0, 20.0, 30.0, 40.0],
        ..Default::default()
    };
    let quad_b = crate::quad::QuadInstance {
        position: [50.0, 60.0, 70.0, 80.0],
        ..Default::default()
    };
    let quad_c = crate::quad::QuadInstance {
        position: [90.0, 100.0, 110.0, 120.0],
        ..Default::default()
    };

    // Call 1: accumulator starts empty -> swap path.
    let mut view1 = tvb.map_instances();
    view1.instances.push(quad_a);
    tvb.accumulate_instances(view1.instances);

    // Call 2: accumulator now non-empty -> extend path, using the
    // capacity-0 vec call 1's swap displaced into the pool. Pushing
    // into it forces its first real allocation.
    let mut view2 = tvb.map_instances();
    view2.instances.push(quad_b);
    tvb.accumulate_instances(view2.instances);

    // Call 3: pool returns whatever call 2 left behind -- now a real,
    // already-allocated vec.
    let mut view3 = tvb.map_instances();
    let ptr3 = view3.instances.as_ptr();
    let cap3 = view3.instances.capacity();
    assert!(
        cap3 > 0,
        "by call 3 the pooled vec must have a real allocation"
    );
    view3.instances.push(quad_c);
    tvb.accumulate_instances(view3.instances);

    // Call 4: must reuse the EXACT SAME allocation call 3 used and
    // returned -- this is the steady-state reuse guarantee.
    let view4 = tvb.map_instances();
    assert_eq!(
        view4.instances.as_ptr(),
        ptr3,
        "call 4 must reuse call 3's pooled vec (same pointer)"
    );
    assert_eq!(
        view4.instances.capacity(),
        cap3,
        "call 4 must reuse call 3's pooled vec (same capacity)"
    );

    // Assert that all three calls' quads made it to the accumulator,
    // in order.
    assert_eq!(
        tvb.instance_count(),
        3,
        "Accumulator should have 3 instances from calls 1, 2 and 3"
    );

    let acc_instances = tvb.instances.borrow();
    assert_eq!(
        acc_instances[0].position,
        [10.0, 20.0, 30.0, 40.0],
        "First quad's position should match"
    );
    assert_eq!(
        acc_instances[1].position,
        [50.0, 60.0, 70.0, 80.0],
        "Second quad's position should match"
    );
    assert_eq!(
        acc_instances[2].position,
        [90.0, 100.0, 110.0, 120.0],
        "Third quad's position should match"
    );
}

/// Test that reentrant nested calls on the same TripleVertexBuffer
/// get different buffers and don't lose data.
///
/// This exercises the REAL `TripleVertexBuffer::map_instances` and
/// `TripleVertexBuffer::accumulate_instances` methods in the exact
/// reentrancy pattern that occurs in production (see box_model.rs:844's
/// recursive `render_element` calls).
///
/// A naive implementation that uses a single shared Vec with
/// `std::mem::take` (or similar) would FAIL this test because the inner
/// call would steal the outer call's in-progress Vec, discarding its data.
/// The pooling implementation passes because each call gets its own
/// exclusively-owned Vec from the pool.
#[test]
fn test_reentrant_calls_use_different_buffers() {
    let tvb = TripleVertexBuffer::new(vec![], 100);

    // Outer call: get a buffer and add distinguishable quads
    let mut outer = tvb.map_instances();
    let outer_ptr = outer.instances.as_ptr();
    let outer_cap = outer.instances.capacity();
    for i in 0..2 {
        let quad = crate::quad::QuadInstance {
            position: [
                100.0 + i as f32,
                200.0 + i as f32,
                300.0 + i as f32,
                400.0 + i as f32,
            ],
            ..Default::default()
        };
        outer.instances.push(quad);
    }

    // Inner call (while outer is still alive and un-accumulated):
    // This simulates the reentrancy from box_model.rs where a child element's
    // render_element is called while the parent's with_quad_allocator is still active.
    let mut inner = tvb.map_instances();
    let inner_ptr = inner.instances.as_ptr();
    let inner_cap = inner.instances.capacity();

    // CRITICAL: outer and inner must be DIFFERENT Vecs
    assert_ne!(
            outer_ptr, inner_ptr,
            "Outer and inner calls must use different Vecs. A naive shared-Vec implementation fails here."
        );
    assert_eq!(
        outer_cap, inner_cap,
        "Both should have the same capacity (from TripleVertexBuffer.capacity)"
    );

    // Add distinguishable quads to inner
    for i in 0..3 {
        let quad = crate::quad::QuadInstance {
            position: [
                500.0 + i as f32,
                600.0 + i as f32,
                700.0 + i as f32,
                800.0 + i as f32,
            ],
            ..Default::default()
        };
        inner.instances.push(quad);
    }

    // Accumulate inner first (this is what happens in real recursion:
    // the inner call finishes before the outer one)
    tvb.accumulate_instances(inner.instances);

    // Accumulate outer
    tvb.accumulate_instances(outer.instances);

    // Verify ALL instances from both calls survived
    assert_eq!(
        tvb.instance_count(),
        5,
        "Accumulator should have 5 instances total (2 outer + 3 inner)"
    );

    // Verify the actual data (not just count) to catch corruption bugs
    let acc_instances = tvb.instances.borrow();
    let mut found_outer = [false; 2];
    let mut found_inner = [false; 3];

    for instance in acc_instances.iter() {
        // Check for outer quads using exact position matching
        if instance.position == [100.0, 200.0, 300.0, 400.0] {
            found_outer[0] = true;
        } else if instance.position == [101.0, 201.0, 301.0, 401.0] {
            found_outer[1] = true;
        }
        // Check for inner quads using exact position matching
        else if instance.position == [500.0, 600.0, 700.0, 800.0] {
            found_inner[0] = true;
        } else if instance.position == [501.0, 601.0, 701.0, 801.0] {
            found_inner[1] = true;
        } else if instance.position == [502.0, 602.0, 702.0, 802.0] {
            found_inner[2] = true;
        }
    }

    assert!(
        found_outer.iter().all(|&x| x),
        "Not all outer quads found in accumulator"
    );
    assert!(
        found_inner.iter().all(|&x| x),
        "Not all inner quads found in accumulator"
    );
}

/// Test that the FIRST `accumulate_instances` call on an empty
/// accumulator takes the swap path: the scratch Vec becomes the
/// accumulator's backing storage directly (same pointer), rather than
/// being copied element-by-element into a separate accumulator Vec. A
/// regression to unconditional `extend` would fail this: the
/// accumulator would then have its own, differently-allocated storage.
#[test]
fn test_first_accumulate_swaps_empty_accumulator() {
    let tvb = TripleVertexBuffer::new(vec![], 100);

    let mut view = tvb.map_instances();
    let quad = crate::quad::QuadInstance {
        position: [1.0, 2.0, 3.0, 4.0],
        ..Default::default()
    };
    view.instances.push(quad);
    let scratch_ptr = view.instances.as_ptr();

    tvb.accumulate_instances(view.instances);

    let acc_ptr = tvb.instances.borrow().as_ptr();
    assert_eq!(
        acc_ptr, scratch_ptr,
        "swap path: accumulator's storage must be the scratch vec's own allocation"
    );
}

/// Test that a SECOND `accumulate_instances` call (accumulator already
/// non-empty) takes the extend path: the accumulator keeps its own
/// storage and the incoming quads are appended, not swapped in. Also
/// verifies append order across three calls, exercising both the swap
/// (first call) and extend (later calls) paths in sequence.
#[test]
fn test_extend_path_appends_in_order_after_swap_path() {
    let tvb = TripleVertexBuffer::new(vec![], 8);

    let mut v1 = tvb.map_instances();
    let acc_ptr_before = tvb.instances.borrow().as_ptr();
    v1.instances.push(crate::quad::QuadInstance {
        position: [1.0, 0.0, 0.0, 0.0],
        ..Default::default()
    });
    tvb.accumulate_instances(v1.instances);
    let acc_ptr_after_first = tvb.instances.borrow().as_ptr();
    assert_ne!(
        acc_ptr_before, acc_ptr_after_first,
        "first call swaps in a new (non-empty) accumulator allocation"
    );

    let mut v2 = tvb.map_instances();
    v2.instances.push(crate::quad::QuadInstance {
        position: [2.0, 0.0, 0.0, 0.0],
        ..Default::default()
    });
    tvb.accumulate_instances(v2.instances);
    assert_eq!(
        tvb.instances.borrow().as_ptr(),
        acc_ptr_after_first,
        "extend path must keep the accumulator's own storage"
    );

    let mut v3 = tvb.map_instances();
    v3.instances.push(crate::quad::QuadInstance {
        position: [3.0, 0.0, 0.0, 0.0],
        ..Default::default()
    });
    tvb.accumulate_instances(v3.instances);

    let positions: Vec<f32> = tvb
        .instances
        .borrow()
        .iter()
        .map(|i| i.position[0])
        .collect();
    assert_eq!(
        positions,
        vec![1.0, 2.0, 3.0],
        "instances must appear in call order regardless of swap-vs-extend path"
    );
}

/// Test that `take_instances_for_wire` still hands over exactly what
/// was accumulated and leaves a fresh, empty accumulator behind (no
/// `pool`, so the replacement is a plain empty `Vec`).
#[test]
fn test_take_instances_for_wire_transfers_and_resets() {
    let tvb = TripleVertexBuffer::new(vec![], 8);

    let mut view = tvb.map_instances();
    view.instances.push(crate::quad::QuadInstance {
        position: [9.0, 9.0, 9.0, 9.0],
        ..Default::default()
    });
    tvb.accumulate_instances(view.instances);
    assert_eq!(tvb.instance_count(), 1);

    let taken = tvb.take_instances_for_wire(None);
    assert_eq!(taken.len(), 1, "taken vec has the accumulated instance");
    assert_eq!(taken[0].position, [9.0, 9.0, 9.0, 9.0]);
    assert_eq!(
        tvb.instance_count(),
        0,
        "accumulator is replaced with an empty vec after taking"
    );

    // The replacement accumulator is empty, so the next accumulate call
    // still takes the swap path rather than erroring or leaking data.
    let mut view2 = tvb.map_instances();
    view2.instances.push(crate::quad::QuadInstance {
        position: [7.0, 7.0, 7.0, 7.0],
        ..Default::default()
    });
    tvb.accumulate_instances(view2.instances);
    assert_eq!(tvb.instance_count(), 1);
    assert_eq!(tvb.instances.borrow()[0].position, [7.0, 7.0, 7.0, 7.0]);
}

/// Test that `clear_quad_allocation` empties the accumulator (so the
/// next `accumulate_instances` call takes the swap path again) without
/// otherwise disturbing the scratch pool.
#[test]
fn test_clear_quad_allocation_empties_accumulator() {
    let tvb = TripleVertexBuffer::new(vec![], 8);

    let mut view = tvb.map_instances();
    view.instances.push(crate::quad::QuadInstance::default());
    tvb.accumulate_instances(view.instances);
    assert_eq!(tvb.instance_count(), 1);

    tvb.clear_quad_allocation();
    assert_eq!(tvb.instance_count(), 0, "clear empties the accumulator");

    // Next accumulate call sees an empty accumulator again -> swap path.
    let mut view2 = tvb.map_instances();
    view2.instances.push(crate::quad::QuadInstance {
        position: [5.0, 5.0, 5.0, 5.0],
        ..Default::default()
    });
    let scratch_ptr = view2.instances.as_ptr();
    tvb.accumulate_instances(view2.instances);
    assert_eq!(
        tvb.instances.borrow().as_ptr(),
        scratch_ptr,
        "post-clear accumulate takes the swap path again"
    );
}

/// Test that the scratch pool does not grow without bound across many
/// sequential (non-nested) map/accumulate cycles: each cycle pops at
/// most one Vec and pushes back exactly one, so pool size must stay
/// bounded by the maximum concurrent nesting depth (1, here), never by
/// the number of frames/calls.
#[test]
fn test_scratch_pool_size_bounded() {
    let tvb = TripleVertexBuffer::new(vec![], 8);

    for i in 0..50 {
        let mut view = tvb.map_instances();
        view.instances.push(crate::quad::QuadInstance {
            position: [i as f32, 0.0, 0.0, 0.0],
            ..Default::default()
        });
        tvb.accumulate_instances(view.instances);
        assert!(
            tvb.scratch_pool.borrow().len() <= 1,
            "pool must not grow past the concurrent-nesting depth (iteration {})",
            i
        );
    }
}
