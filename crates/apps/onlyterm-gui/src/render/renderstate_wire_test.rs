use super::TripleVertexBuffer;
use crate::quad::QuadInstance;
use onlyterm_gpu_render::wire;

#[test]
fn wire_frame_takes_the_accumulator_without_copying_instances() {
    let layer = TripleVertexBuffer::new(Vec::new(), 2);
    let pool = wire::new_draw_pool();
    layer.accumulate_instances(vec![QuadInstance::default(); 2]);
    let original = layer.instances.borrow().as_ptr();
    let first = layer.take_instances_for_wire(Some(&pool));
    assert_eq!(first.as_ptr(), original);
    assert_eq!(first.len(), 2);
    assert_eq!(layer.instance_count(), 0);

    let next = QuadInstance {
        position: [1., 2., 3., 4.],
        ..QuadInstance::default()
    };
    layer.accumulate_instances(vec![next]);
    assert_eq!(first[0].position, QuadInstance::default().position);
    wire::pool_return(&pool, first);
    let second = layer.take_instances_for_wire(Some(&pool));
    assert_eq!(second[0].position, [1., 2., 3., 4.]);
    assert_eq!(layer.instances.borrow().as_ptr(), original);
    assert_eq!(layer.instance_count(), 0);
}

#[test]
fn wire_frame_without_pool_still_transfers_owned_storage() {
    let layer = TripleVertexBuffer::new(Vec::new(), 1);
    layer.accumulate_instances(vec![QuadInstance::default()]);
    let ptr = layer.instances.borrow().as_ptr();
    let frame = layer.take_instances_for_wire(None);
    assert_eq!(frame.as_ptr(), ptr);
    assert_eq!(frame.len(), 1);
    assert_eq!(layer.instance_count(), 0);
}
