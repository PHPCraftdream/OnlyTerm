//! Test-only reference for the pre-flattening `Vec<Vec<Info>>` clustering
//! algorithm (task C3.3), kept solely to differential-test the flat
//! `Vec<Info>` + `group_starts` replacement (`cluster_shaped_infos` in the
//! parent module) against it.
//!
//! `assert_clustering_matches_reference` is wired into every real
//! `do_shape` call in test builds (see the `#[cfg(test)]` block
//! immediately after `cluster_shaped_infos` is called there), so every
//! shaping test in this crate -- ASCII, ligatures, Hebrew/Arabic RTL,
//! combining marks, CJK, emoji ZWJ, fallback-exhausted runs -- is also a
//! differential test for this grouping logic, not just the handful of
//! strings shaped directly by `clustering_differential_strings.rs`.
use super::*;

/// Exact pre-flattening port of the clustering loop `cluster_shaped_infos`
/// (in the parent module) replaced: builds one inner `Vec<Info>` per
/// cluster group instead of a flat `Vec<Info>` + `group_starts` index.
/// Any divergence between this and `cluster_shaped_infos` is a real
/// behavior change the differential check below will catch.
fn cluster_shaped_infos_reference(
    rb_infos: &[rustybuzz::GlyphInfo],
    positions: &[rustybuzz::GlyphPosition],
    cluster_resolver: &mut ClusterResolver,
    range: &Range<usize>,
    no_more_fallbacks: bool,
    scale: f64,
) -> Vec<Vec<Info>> {
    let scaled_advance = |raw: i32| -> f64 { (raw as f64 * scale).round() };
    let scaled_offset = |raw: i32| -> f64 { raw as f64 * scale };

    let mut info_clusters: Vec<Vec<Info>> = Vec::with_capacity(rb_infos.len());

    for (info, pos) in rb_infos.iter().zip(positions.iter()) {
        let cluster_info = match cluster_resolver.get_mut(info.cluster as usize + range.start) {
            Some(i) => i,
            None => panic!(
                "expected cluster info.cluster {} to be in cluster_resolver",
                info.cluster
            ),
        };
        let len = cluster_info.byte_len;

        let mut info = Info {
            cluster: cluster_info.start,
            len,
            codepoint: info.glyph_id,
            x_advance: scaled_advance(pos.x_advance),
            y_advance: scaled_advance(pos.y_advance),
            x_offset: scaled_offset(pos.x_offset),
            y_offset: scaled_offset(pos.y_offset),
        };

        if info.codepoint == 0 && !no_more_fallbacks {
            cluster_info.incomplete = true;
        }

        if let Some(ref mut cluster) = info_clusters.last_mut() {
            if info.codepoint == 0 && !no_more_fallbacks {
                let prior = cluster.last_mut().unwrap();
                if prior.codepoint == 0 || prior.cluster == info.cluster {
                    if prior.cluster + prior.len == info.cluster {
                        prior.len += info.len;
                        continue;
                    } else if info.cluster + info.len == prior.cluster {
                        std::mem::swap(&mut info, prior);
                        prior.len += info.len;
                        continue;
                    } else if info.cluster + info.len == prior.cluster + prior.len {
                        continue;
                    }
                }
            }

            if cluster.last().unwrap().cluster == info.cluster {
                cluster.push(info);
                continue;
            }
        }
        info_clusters.push(vec![info]);
    }

    info_clusters
}

fn infos_equal(a: &Info, b: &Info) -> bool {
    a.cluster == b.cluster
        && a.len == b.len
        && a.codepoint == b.codepoint
        && a.x_advance == b.x_advance
        && a.y_advance == b.y_advance
        && a.x_offset == b.x_offset
        && a.y_offset == b.y_offset
}

/// Re-groups `rb_infos`/`positions` with the old nested-Vec reference
/// algorithm, against a freshly built, independent `ClusterResolver` (so
/// this cannot disturb the production `cluster_resolver` the caller keeps
/// using afterwards), and asserts the result is identical -- group for
/// group, entry for entry -- to the flat `(flat_infos, group_starts)` the
/// production `cluster_shaped_infos` just produced.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assert_clustering_matches_reference(
    rb_infos: &[rustybuzz::GlyphInfo],
    positions: &[rustybuzz::GlyphPosition],
    presentation_width: Option<&PresentationWidth>,
    s: &str,
    range: &Range<usize>,
    no_more_fallbacks: bool,
    scale: f64,
    flat_infos: &[Info],
    group_starts: &[usize],
) {
    let mut reference_resolver = ClusterResolver::new(presentation_width);
    reference_resolver.build(rb_infos, s, range);
    let reference_groups = cluster_shaped_infos_reference(
        rb_infos,
        positions,
        &mut reference_resolver,
        range,
        no_more_fallbacks,
        scale,
    );

    assert_eq!(
        reference_groups.len(),
        group_starts.len(),
        "group count mismatch: reference(nested)={} new(flat)={} for text {:?} range {:?}",
        reference_groups.len(),
        group_starts.len(),
        s,
        range,
    );

    for (gi, ref_group) in reference_groups.iter().enumerate() {
        let start = group_starts[gi];
        let end = group_starts
            .get(gi + 1)
            .copied()
            .unwrap_or(flat_infos.len());
        let new_group = &flat_infos[start..end];
        assert_eq!(
            ref_group.len(),
            new_group.len(),
            "group {} length mismatch: reference={:?} new={:?} for text {:?}",
            gi,
            ref_group,
            new_group,
            s
        );
        for (ref_info, new_info) in ref_group.iter().zip(new_group.iter()) {
            assert!(
                infos_equal(ref_info, new_info),
                "group {} entry mismatch: reference={:?} new={:?} for text {:?}",
                gi,
                ref_info,
                new_info,
                s
            );
        }
    }
}
