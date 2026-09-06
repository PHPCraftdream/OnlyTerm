use super::*;

#[test]
fn clone_shares_storage_until_mutation() {
    let line: Line = "shared snapshot".into();
    let mut snapshot = line.clone();
    match (&line.cells, &snapshot.cells) {
        (CellStorage::V(original), CellStorage::V(clone)) => {
            assert!(Arc::ptr_eq(original, clone));
        }
        _ => panic!("text lines should use vector storage"),
    }

    snapshot.set_cell(0, Cell::new('X', CellAttributes::blank()), SEQ_ZERO + 1);
    assert_eq!(line.as_str(), "shared snapshot");
    assert_eq!(snapshot.as_str(), "Xhared snapshot");
    match (&line.cells, &snapshot.cells) {
        (CellStorage::V(original), CellStorage::V(clone)) => {
            assert!(!Arc::ptr_eq(original, clone));
        }
        _ => panic!("mutation should retain vector storage"),
    }
}

#[test]
fn compressed_clone_detaches_on_materialization() {
    let line: Line = "compressed snapshot".into();
    let mut compressed = line.clone();
    compressed.compress_for_scrollback();
    let mut snapshot = compressed.clone();
    match (&compressed.cells, &snapshot.cells) {
        (CellStorage::C(original), CellStorage::C(clone)) => {
            assert!(Arc::ptr_eq(original, clone));
        }
        _ => panic!("compression should use clustered storage"),
    }

    snapshot.set_cell(0, Cell::new('X', CellAttributes::blank()), SEQ_ZERO + 1);
    assert_eq!(compressed.as_str(), "compressed snapshot");
    assert_eq!(snapshot.as_str(), "Xompressed snapshot");
    assert!(matches!(snapshot.cells, CellStorage::V(_)));
}

#[test]
fn cloned_zone_cache_detaches_with_mutation() {
    let mut line: Line = "zone cache".into();
    let _ = line.semantic_zone_ranges();
    let mut snapshot = line.clone();
    assert!(Arc::ptr_eq(&line.zones, &snapshot.zones));

    snapshot.set_cell(0, Cell::new('X', CellAttributes::blank()), SEQ_ZERO + 1);
    assert!(!Arc::ptr_eq(&line.zones, &snapshot.zones));
    assert_eq!(line.as_str(), "zone cache");
}

#[cfg(feature = "use_serde")]
#[test]
fn serde_roundtrip_keeps_legacy_line_shape() {
    let mut line = Line::from_text(
        "serde snapshot",
        &CellAttributes::blank(),
        SEQ_ZERO + 7,
        None,
    );
    let _ = line.semantic_zone_ranges();

    let encoded = serde_json::to_string(&line).unwrap();
    let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert!(value["cells"]["V"]["cells"].is_array());
    assert!(value["zones"].is_array());

    let mut decoded: Line = serde_json::from_str(&encoded).unwrap();
    assert_eq!(line, decoded);
    assert_eq!(line.as_str(), decoded.as_str());
    assert_eq!(line.semantic_zone_ranges(), decoded.semantic_zone_ranges());
    assert_eq!(encoded, serde_json::to_string(&decoded).unwrap());
}
