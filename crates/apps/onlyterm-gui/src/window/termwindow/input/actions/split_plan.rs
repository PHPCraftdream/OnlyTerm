use onlyterm_mux::pane::PaneId;
use onlyterm_mux::tab::{SplitDirection, SplitRequest, SplitSize};
use std::future::Future;

pub(super) fn plan_three_way_split(
    total: usize,
    direction: SplitDirection,
) -> Option<[SplitRequest; 2]> {
    if total < 5 {
        return None;
    }

    let cells = total - 2;
    let third = cells / 3;
    let remainder = cells % 3;
    let middle = third + usize::from(remainder > 1);
    let last = third;

    Some([
        SplitRequest {
            direction,
            target_is_second: true,
            top_level: false,
            size: SplitSize::Cells(middle + 1 + last),
        },
        SplitRequest {
            direction,
            target_is_second: true,
            top_level: false,
            size: SplitSize::Cells(last),
        },
    ])
}

/// cancel-safe: no; the caller must keep this task alive through rollback.
pub(super) async fn execute_three_way<F, Fut, R, RFut>(
    source: PaneId,
    requests: [SplitRequest; 2],
    mut split: F,
    rollback: R,
) -> anyhow::Result<PaneId>
where
    F: FnMut(PaneId, SplitRequest) -> Fut,
    Fut: Future<Output = anyhow::Result<PaneId>>,
    R: FnOnce(PaneId) -> RFut,
    RFut: Future<Output = anyhow::Result<()>>,
{
    let first_new = split(source, requests[0]).await?;
    match split(first_new, requests[1]).await {
        Ok(last_new) => Ok(last_new),
        Err(error) => match rollback(first_new).await {
            Ok(()) => Err(error),
            Err(rollback_error) => {
                Err(error.context(format!("rollback failed: {rollback_error:#}")))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::execute_three_way;
    use onlyterm_mux::tab::{SplitDirection, SplitRequest, SplitSize};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn requests(total: usize, direction: SplitDirection) -> [SplitRequest; 2] {
        super::plan_three_way_split(total, direction).expect("dimension can fit three panes")
    }

    #[test]
    fn three_way_requests_keep_original_first_and_balance_remainder() {
        for (total, first_new, last_new) in
            [(5, 3, 1), (6, 3, 1), (7, 4, 1), (8, 5, 2), (100, 66, 32)]
        {
            for direction in [SplitDirection::Horizontal, SplitDirection::Vertical] {
                let [first, second] = requests(total, direction);
                assert_eq!(
                    first,
                    SplitRequest {
                        direction,
                        target_is_second: true,
                        top_level: false,
                        size: SplitSize::Cells(first_new),
                    }
                );
                assert_eq!(
                    second,
                    SplitRequest {
                        direction,
                        target_is_second: true,
                        top_level: false,
                        size: SplitSize::Cells(last_new),
                    }
                );
            }
        }
    }

    #[test]
    fn three_way_requests_reject_dimensions_without_room_for_two_separators() {
        for total in 0..5 {
            assert!(
                super::plan_three_way_split(total, SplitDirection::Horizontal).is_none(),
                "dimension {} must not spawn any pane",
                total
            );
        }
    }

    #[test]
    fn three_way_requests_partition_every_supported_dimension() {
        for total in 5..=200 {
            let [first, second] = requests(total, SplitDirection::Horizontal);
            let (SplitSize::Cells(right_region), SplitSize::Cells(last)) =
                (first.size, second.size)
            else {
                panic!("three-way plan must use exact cell sizes");
            };
            let original = total - right_region - 1;
            let middle = right_region - last - 1;
            let sizes = [original, middle, last];
            assert_eq!(sizes.iter().sum::<usize>() + 2, total);
            assert!(sizes.iter().all(|size| *size >= 1));
            assert!(sizes.iter().max().unwrap() - sizes.iter().min().unwrap() <= 1);
        }
    }

    #[test]
    fn three_way_awaits_the_first_new_pane_before_targeting_the_second() {
        let requests = requests(8, SplitDirection::Horizontal);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let rollbacks = Rc::new(RefCell::new(Vec::new()));
        let split_calls = Rc::clone(&calls);
        let rollback_calls = Rc::clone(&rollbacks);
        let result = onlyterm_promise::spawn::block_on(execute_three_way(
            10,
            requests,
            move |pane_id, request| {
                let calls = Rc::clone(&split_calls);
                async move {
                    calls.borrow_mut().push((pane_id, request));
                    Ok(if pane_id == 10 { 20 } else { 30 })
                }
            },
            move |pane_id| async move {
                rollback_calls.borrow_mut().push(pane_id);
                Ok(())
            },
        ))
        .unwrap();
        assert_eq!(result, 30);
        assert_eq!(*calls.borrow(), [(10, requests[0]), (20, requests[1])]);
        assert!(rollbacks.borrow().is_empty());
    }

    #[test]
    fn three_way_failure_rolls_back_the_first_new_pane() {
        let requests = requests(8, SplitDirection::Vertical);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let rollbacks = Rc::new(RefCell::new(Vec::new()));
        let split_calls = Rc::clone(&calls);
        let rollback_calls = Rc::clone(&rollbacks);
        let result = onlyterm_promise::spawn::block_on(execute_three_way(
            10,
            requests,
            move |pane_id, _| {
                let calls = Rc::clone(&split_calls);
                async move {
                    calls.borrow_mut().push(pane_id);
                    if pane_id == 10 {
                        Ok(20)
                    } else {
                        Err(anyhow::anyhow!("second split failed"))
                    }
                }
            },
            move |pane_id| async move {
                rollback_calls.borrow_mut().push(pane_id);
                Ok(())
            },
        ));
        assert!(result.is_err());
        assert_eq!(*calls.borrow(), [10, 20]);
        assert_eq!(*rollbacks.borrow(), [20]);
    }

    #[test]
    fn three_way_reports_a_failed_rollback_as_well_as_the_second_split() {
        let requests = requests(8, SplitDirection::Horizontal);
        let result = onlyterm_promise::spawn::block_on(execute_three_way(
            10,
            requests,
            |pane_id, _| async move {
                if pane_id == 10 {
                    Ok(20)
                } else {
                    Err(anyhow::anyhow!("second split failed"))
                }
            },
            |_pane_id| async { Err(anyhow::anyhow!("remote rollback failed")) },
        ));
        let error = result.expect_err("second split must fail");
        let message = format!("{error:#}");
        assert!(message.contains("second split failed"), "{}", message);
        assert!(message.contains("remote rollback failed"), "{}", message);
    }
}
