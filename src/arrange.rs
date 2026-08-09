//! Pure logic behind phase 8: where an unplaced note proposes to stand
//! (adr/2026-08-auto-place-strongest-link-ring.md), and the explicit
//! cluster arrange (adr/2026-08-arrange-cluster-command.md). Everything
//! deterministic and bounded — for loops with structural bounds, never a
//! search to convergence.

/// The ring walk's cell pitch — the fallback grid's — and its reach.
pub const SLOT_W: f64 = 192.0;
pub const SLOT_H: f64 = 96.0;
pub const RING_CAP: i32 = 8;

/// The proposal for an unplaced note: the first free ring cell beside the
/// anchor its links reach most strongly, or `None` when no link reaches an
/// anchor — the caller falls back to the origin grid. `occupied` is every
/// position already resolved this derivation, so proposals never stack.
pub fn auto_place(
    id: &str,
    edges: &[(String, String)],
    anchors: &[(String, (f64, f64))],
    occupied: &[(f64, f64)],
) -> Option<(f64, f64)> {
    let anchor = strongest(id, edges, anchors)?;
    Some(ring_slot(anchor, occupied))
}

/// The anchor with the most edges to `id`, either direction; ties break to
/// the smallest anchor id — deterministic whatever order the anchors come
/// in.
fn strongest(
    id: &str,
    edges: &[(String, String)],
    anchors: &[(String, (f64, f64))],
) -> Option<(f64, f64)> {
    let mut best: Option<(usize, &str, (f64, f64))> = None;
    for (anchor, at) in anchors {
        let count = edges
            .iter()
            .filter(|(source, target)| {
                (source == id && target == anchor)
                    || (source == anchor && target == id)
            })
            .count();
        if count == 0 {
            continue;
        }
        let stronger = best.is_none_or(|(top, leader, _)| {
            count > top || (count == top && anchor.as_str() < leader)
        });
        if stronger {
            best = Some((count, anchor, *at));
        }
    }
    best.map(|(_, _, at)| at)
}

/// The first free cell on a growing ring around the anchor: rings 1..=8,
/// perimeter cells in row-major order. A cell is free when no occupied
/// point stands within one pitch of it. A saturated neighbourhood answers
/// the last cell examined — total and bounded.
fn ring_slot(anchor: (f64, f64), occupied: &[(f64, f64)]) -> (f64, f64) {
    let mut last = anchor;
    for ring in 1..=RING_CAP {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() != ring && dy.abs() != ring {
                    continue;
                }
                let cell = (
                    anchor.0 + f64::from(dx) * SLOT_W,
                    anchor.1 + f64::from(dy) * SLOT_H,
                );
                let free = !occupied.iter().any(|at| {
                    (at.0 - cell.0).abs() < SLOT_W
                        && (at.1 - cell.1).abs() < SLOT_H
                });
                if free {
                    return cell;
                }
                last = cell;
            }
        }
    }
    last
}

/// The undirected connected component of `start` — the arrange command's
/// scope. An explicit stack popped inside a for loop whose bound is the
/// node count (each node is pushed at most once), so neither recursion nor
/// an unbounded loop; membership is a linear scan, fine at vault scale.
pub fn component(start: &str, edges: &[(String, String)]) -> Vec<String> {
    let mut members = vec![start.to_string()];
    let mut stack = vec![start.to_string()];
    // every node the edge list can name, plus the start
    let node_bound = edges.len() * 2 + 1;
    for _ in 0..node_bound {
        let Some(node) = stack.pop() else { break };
        for (source, target) in edges {
            let neighbour = if source == &node {
                target
            } else if target == &node {
                source
            } else {
                continue;
            };
            if !members.iter().any(|member| member == neighbour) {
                members.push(neighbour.clone());
                stack.push(neighbour.clone());
            }
        }
    }
    members.sort_unstable();
    members
}

/// The spring pass's frozen constants
/// (adr/2026-08-arrange-cluster-command.md).
pub const ARRANGE_ITERATIONS: usize = 50;
const SPRING_LENGTH: f64 = 240.0;
const SPRING_K: f64 = 0.06;
const REPULSION: f64 = 48_000.0;
const MAX_STEP: f64 = 48.0;

/// One deterministic force-directed pass over a cluster, seeded from the
/// cards' current positions: springs along the cluster's edges, repulsion
/// between every pair, displacement clamped per iteration, hard-capped at
/// `ARRANGE_ITERATIONS` — then it stops wherever it is. No randomness:
/// coincident seeds separate along an index-derived direction.
pub fn arrange(
    ids: &[String],
    edges: &[(String, String)],
    seed: &[(String, (f64, f64))],
) -> Vec<(String, (f64, f64))> {
    let mut points: Vec<(String, (f64, f64))> = ids
        .iter()
        .map(|id| {
            let at = seed
                .iter()
                .find(|(seeded, _)| seeded == id)
                .map(|(_, at)| *at)
                .unwrap_or((0.0, 0.0));
            (id.clone(), at)
        })
        .collect();
    let linked: Vec<(usize, usize)> = edges
        .iter()
        .filter_map(|(source, target)| {
            let a = points.iter().position(|(id, _)| id == source)?;
            let b = points.iter().position(|(id, _)| id == target)?;
            (a != b).then_some((a.min(b), a.max(b)))
        })
        .collect();

    for _ in 0..ARRANGE_ITERATIONS {
        let mut forces = vec![(0.0f64, 0.0f64); points.len()];
        for a in 0..points.len() {
            for b in (a + 1)..points.len() {
                let (ux, uy, distance) =
                    separation(points[a].1, points[b].1, a, b);
                let push = REPULSION / (distance * distance);
                forces[a].0 -= push * ux;
                forces[a].1 -= push * uy;
                forces[b].0 += push * ux;
                forces[b].1 += push * uy;
            }
        }
        for (a, b) in &linked {
            let (ux, uy, distance) =
                separation(points[*a].1, points[*b].1, *a, *b);
            let pull = SPRING_K * (distance - SPRING_LENGTH);
            forces[*a].0 += pull * ux;
            forces[*a].1 += pull * uy;
            forces[*b].0 -= pull * ux;
            forces[*b].1 -= pull * uy;
        }
        for (point, force) in points.iter_mut().zip(&forces) {
            let norm = (force.0 * force.0 + force.1 * force.1).sqrt();
            let clamp = if norm > MAX_STEP {
                MAX_STEP / norm
            } else {
                1.0
            };
            point.1.0 += force.0 * clamp;
            point.1.1 += force.1 * clamp;
        }
    }
    points
}

/// The unit direction a → b and their distance, never degenerate: two
/// coincident points separate along an index-derived direction instead of
/// dividing by zero.
fn separation(
    a: (f64, f64),
    b: (f64, f64),
    rank_a: usize,
    rank_b: usize,
) -> (f64, f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < 1.0 {
        let spread = (rank_b - rank_a) as f64;
        let norm = (1.0 + spread * spread).sqrt();
        return (1.0 / norm, spread / norm, 1.0);
    }
    (dx / distance, dy / distance, distance)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn link(source: &str, target: &str) -> (String, String) {
        (source.to_string(), target.to_string())
    }

    fn anchor(id: &str, x: f64, y: f64) -> (String, (f64, f64)) {
        (id.to_string(), (x, y))
    }

    #[test]
    fn an_unplaced_note_lands_on_the_ring_beside_its_strongest_link() {
        let edges = [link("new", "a"), link("new", "b"), link("b", "new")];
        let anchors = [anchor("a", 0.0, 0.0), anchor("b", 1000.0, 0.0)];
        // b is reached twice, a once: the ring grows around b, and its
        // first perimeter cell in row-major order is the top-left
        let landed = auto_place("new", &edges, &anchors, &[])
            .expect("a linked note gets a proposal");
        assert_eq!(landed, (1000.0 - SLOT_W, -SLOT_H));
    }

    #[test]
    fn a_strength_tie_breaks_to_the_smallest_anchor_id() {
        let edges = [link("new", "b"), link("new", "a")];
        let anchors = [anchor("b", 500.0, 0.0), anchor("a", 0.0, 0.0)];
        let landed = auto_place("new", &edges, &anchors, &[])
            .expect("a linked note gets a proposal");
        assert_eq!(landed, (-SLOT_W, -SLOT_H), "beside a, not b");

        // and a weaker later candidate never displaces a stronger leader
        let edges = [link("new", "b"), link("b", "new"), link("new", "a")];
        let landed = auto_place("new", &edges, &anchors, &[])
            .expect("a linked note gets a proposal");
        assert_eq!(landed, (500.0 - SLOT_W, -SLOT_H), "b held its lead");
    }

    #[test]
    fn the_ring_walk_takes_the_first_free_cell_in_a_fixed_order() {
        let edges = [link("new", "a")];
        let anchors = [anchor("a", 0.0, 0.0)];
        // the first two row-major perimeter cells are taken: the third wins
        let occupied = [(-SLOT_W, -SLOT_H), (0.0, -SLOT_H)];
        let landed = auto_place("new", &edges, &anchors, &occupied)
            .expect("a linked note gets a proposal");
        assert_eq!(landed, (SLOT_W, -SLOT_H));
    }

    #[test]
    fn a_saturated_neighbourhood_still_answers_within_the_ring_cap() {
        let edges = [link("new", "a")];
        let anchors = [anchor("a", 0.0, 0.0)];
        // every cell of every ring is within a pitch of some occupant: a
        // dense blanket over the whole reach
        let mut blanket = Vec::new();
        for dy in -(RING_CAP + 1)..=(RING_CAP + 1) {
            for dx in -(RING_CAP + 1)..=(RING_CAP + 1) {
                blanket.push((f64::from(dx) * SLOT_W, f64::from(dy) * SLOT_H));
            }
        }
        let landed = auto_place("new", &edges, &anchors, &blanket)
            .expect("still an answer, never a spin");
        // the last cell examined: the outermost ring's bottom-right
        assert_eq!(
            landed,
            (f64::from(RING_CAP) * SLOT_W, f64::from(RING_CAP) * SLOT_H)
        );
    }

    #[test]
    fn a_note_with_only_unanchored_links_gets_no_proposal() {
        let edges = [link("new", "ghost")];
        let anchors = [anchor("a", 0.0, 0.0)];
        assert_eq!(auto_place("new", &edges, &anchors, &[]), None);
        assert_eq!(auto_place("loner", &[], &anchors, &[]), None);
    }

    #[test]
    fn component_crosses_edges_both_ways_and_survives_a_cycle() {
        let edges = [
            link("a", "b"),
            link("c", "b"),
            link("c", "a"),
            link("d", "e"),
        ];
        assert_eq!(component("a", &edges), vec!["a", "b", "c"]);
        assert_eq!(component("e", &edges), vec!["d", "e"]);
        assert_eq!(component("island", &edges), vec!["island"]);
    }

    #[test]
    fn arrange_is_identical_twice_from_the_same_seed() {
        let ids: Vec<String> =
            ["a", "b", "c"].iter().map(|id| id.to_string()).collect();
        let edges = [link("a", "b"), link("b", "c")];
        let seed = [
            anchor("a", 0.0, 0.0),
            anchor("b", 30.0, 10.0),
            anchor("c", 700.0, 500.0),
        ];
        assert_eq!(arrange(&ids, &edges, &seed), arrange(&ids, &edges, &seed));
    }

    #[test]
    fn arrange_moves_linked_cards_toward_rest_and_strangers_apart() {
        let ids: Vec<String> =
            ["a", "b", "c"].iter().map(|id| id.to_string()).collect();
        // the edges beyond the cluster — into it or out of it — spring
        // nothing: only a–b is a spring here
        let edges = [link("a", "b"), link("a", "ghost"), link("ghost", "b")];
        let seed = [
            anchor("a", 0.0, 0.0),
            anchor("b", 900.0, 0.0),
            anchor("c", 40.0, 20.0),
        ];
        let laid = arrange(&ids, &edges, &seed);
        let at = |id: &str| {
            laid.iter()
                .find(|(own, _)| own == id)
                .map(|(_, at)| *at)
                .expect("every id is laid out")
        };
        let gap = |p: (f64, f64), q: (f64, f64)| {
            ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt()
        };
        assert!(
            gap(at("a"), at("b")) < 900.0,
            "the spring pulled the linked pair in: {laid:?}"
        );
        assert!(
            gap(at("a"), at("c")) > gap((0.0, 0.0), (40.0, 20.0)),
            "the strangers repelled: {laid:?}"
        );
        assert!(
            laid.iter()
                .all(|(_, (x, y))| x.is_finite() && y.is_finite()),
            "finite after the cap: {laid:?}"
        );
    }

    #[test]
    fn arrange_from_a_coincident_seed_still_separates_deterministically() {
        let ids: Vec<String> =
            ["a", "b"].iter().map(|id| id.to_string()).collect();
        let edges = [link("a", "b")];
        // b unseeded: both start at the origin
        let seed = [anchor("a", 0.0, 0.0)];
        let laid = arrange(&ids, &edges, &seed);
        assert_ne!(laid[0].1, laid[1].1, "the nudge separated them");
        assert_eq!(
            laid,
            arrange(&ids, &edges, &seed),
            "and deterministically"
        );
        // a self-link is ignored rather than springing a card to itself
        let selfish = [link("a", "a")];
        let alone = arrange(&ids[..1], &selfish, &[anchor("a", 12.0, 34.0)]);
        assert_eq!(alone[0].1, (12.0, 34.0));
    }
}
