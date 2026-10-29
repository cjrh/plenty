use super::*;
use crate::op::{MatchArm, Pattern};

#[test]
fn match_edges_preserve_arm_order_and_empty_arm_continuations() {
    for arm_count in 0..=8 {
        let arms: Arc<[MatchArm]> = (0..arm_count)
            .map(|id| MatchArm {
                pattern: Pattern::Wildcard,
                body: if id % 2 == 0 {
                    Arc::from([])
                } else {
                    Arc::from([Op::UseLoan(id)])
                },
            })
            .collect();
        let ops = [Op::Match(arms), Op::UseLoan(99)];
        let mut nodes = vec![Node::default()];
        let entry = graph(&ops, None, 0, None, None, &mut nodes);
        let edges = nodes[entry].successors.as_slice();
        assert_eq!(edges.len(), arm_count);
        for (id, &edge) in edges.iter().enumerate() {
            if id % 2 == 0 {
                assert_eq!(edge, 1);
            } else {
                assert!(matches!(nodes[edge].op, Some(Op::UseLoan(loan)) if *loan == id));
                assert_eq!(nodes[edge].successors.as_slice(), &[1]);
            }
        }
    }
}

#[test]
fn loop_edges_keep_condition_backedge_and_early_exit_targets() {
    let ops = [
        Op::Loop {
            condition: Arc::from([Op::UseLoan(10)]),
            body: Arc::from([Op::If {
                then: Arc::from([Op::Break]),
                elifs: Arc::from([]),
                otherwise: Arc::from([Op::Continue]),
            }]),
        },
        Op::UseLoan(20),
    ];
    let mut nodes = vec![Node::default()];
    let entry = graph(&ops, None, 0, None, None, &mut nodes);
    assert_eq!(entry, 2);
    let edges: Vec<_> = nodes
        .iter()
        .map(|node| node.successors.as_slice())
        .collect();
    assert_eq!(
        edges,
        [&[][..], &[0], &[7], &[2], &[1], &[4, 3], &[5, 1], &[6]]
    );
}
