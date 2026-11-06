//! Weighted rail graph. All routing is computed at configuration load time.
use anyhow::{ensure, Result};
use ttfx::utils::rng::Rng;

const EPS: f64 = 1e-9;

pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub length: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Source {
    pub edge: usize,
    /// Distance from edge.a, strictly between the two nodes.
    pub offset: f64,
}

pub struct Wave {
    pub source: Source,
    pub distances: Vec<f64>,
    pub reach: f64,
}

pub struct Graph {
    pub nodes: Vec<[f64; 2]>,
    pub edges: Vec<Edge>,
    adjacency: Vec<Vec<(usize, f64)>>,
    pub diameter: f64,
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

impl Graph {
    pub fn new(paths: &[Vec<[f64; 2]>]) -> Result<Self> {
        let mut nodes = Vec::new();
        for &point in paths.iter().flatten() {
            if !nodes.iter().any(|&other| distance(point, other) < EPS) {
                nodes.push(point);
            }
        }
        ensure!(nodes.len() >= 2, "Trace graph needs at least two nodes");
        let mut edges: Vec<Edge> = Vec::new();
        // Split each rail at every explicitly supplied node on that rail.
        // Repeated/reversed walks in older configs deduplicate to the same graph.
        for path in paths {
            ensure!(path.len() >= 2, "Trace paths require at least two points");
            for pair in path.windows(2) {
                let [a, b] = [pair[0], pair[1]];
                let length = distance(a, b);
                if length < EPS {
                    continue;
                }
                let mut cuts = Vec::new();
                for (index, &point) in nodes.iter().enumerate() {
                    let t = ((point[0] - a[0]) * (b[0] - a[0]) + (point[1] - a[1]) * (b[1] - a[1]))
                        / length.powi(2);
                    let projected = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                    if (-EPS..=1.0 + EPS).contains(&t) && distance(point, projected) < EPS {
                        cuts.push((t, index));
                    }
                }
                cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
                for pair in cuts.windows(2) {
                    let (a, b) = (pair[0].1, pair[1].1);
                    if a != b
                        && !edges.iter().any(|edge| {
                            (edge.a == a && edge.b == b) || (edge.a == b && edge.b == a)
                        })
                    {
                        edges.push(Edge {
                            a,
                            b,
                            length: distance(nodes[a], nodes[b]),
                        });
                    }
                }
            }
        }
        ensure!(
            !edges.is_empty() && edges.len() <= 4096,
            "Invalid trace rail count"
        );
        let mut adjacency = vec![Vec::new(); nodes.len()];
        for edge in &edges {
            adjacency[edge.a].push((edge.b, edge.length));
            adjacency[edge.b].push((edge.a, edge.length));
        }
        let mut graph = Self {
            nodes,
            edges,
            adjacency,
            diameter: 0.0,
        };
        for node in 0..graph.nodes.len() {
            let distances = graph.shortest_distances(&[(node, 0.0)]);
            ensure!(
                distances.iter().all(|d| d.is_finite()),
                "Trace rails must form one connected graph"
            );
            graph.diameter = graph
                .diameter
                .max(distances.into_iter().fold(0.0, f64::max));
        }
        Ok(graph)
    }

    // Dijkstra over the small, bounded icon graph; never run in the frame loop.
    fn shortest_distances(&self, seeds: &[(usize, f64)]) -> Vec<f64> {
        let mut distances = vec![f64::INFINITY; self.nodes.len()];
        let mut visited = vec![false; self.nodes.len()];
        for &(node, value) in seeds {
            distances[node] = value;
        }
        for _ in 0..self.nodes.len() {
            let next = (0..self.nodes.len())
                .filter(|&node| !visited[node])
                .min_by(|&a, &b| distances[a].total_cmp(&distances[b]));
            let Some(node) = next else { break };
            if !distances[node].is_finite() {
                break;
            }
            visited[node] = true;
            for &(other, length) in &self.adjacency[node] {
                distances[other] = distances[other].min(distances[node] + length);
            }
        }
        distances
    }

    pub fn wave(&self, source: Source) -> Wave {
        let edge = &self.edges[source.edge];
        let distances = self.shortest_distances(&[
            (edge.a, source.offset),
            (edge.b, edge.length - source.offset),
        ]);
        let mut reach: f64 = 0.0;
        for (index, edge) in self.edges.iter().enumerate() {
            let from_a = distances[edge.a];
            let from_b = distances[edge.b];
            if index == source.edge {
                // Split the starting rail at the interior source as a virtual node.
                reach = reach
                    .max((from_a + source.offset) / 2.0)
                    .max((from_b + edge.length - source.offset) / 2.0);
            } else {
                // The two fronts meet inside this rail. Shortest-path distances
                // obey |da-db| <= length, so this is its last arrival time.
                reach = reach.max((from_a + from_b + edge.length) / 2.0);
            }
        }
        Wave {
            source,
            distances,
            reach,
        }
    }

    pub fn sources(&self, rng: &mut Rng, clearance: f64) -> Result<Vec<Source>> {
        let available: f64 = self
            .edges
            .iter()
            .map(|edge| (edge.length - 2.0 * clearance).max(0.0))
            .sum();
        ensure!(
            available > EPS,
            "No rail has enough interior clearance for a random start"
        );
        let mut sources: Vec<Source> = Vec::new();
        for phase in 0..64 {
            loop {
                let mut sample = rng.random() * available;
                let mut source = None;
                for (edge_index, edge) in self.edges.iter().enumerate() {
                    let eligible = (edge.length - 2.0 * clearance).max(0.0);
                    if sample < eligible {
                        source = Some(Source {
                            edge: edge_index,
                            offset: clearance + sample,
                        });
                        break;
                    }
                    sample -= eligible;
                }
                let Some(source) = source else { continue };
                let same = |other: Source| {
                    other.edge == source.edge && (other.offset - source.offset).abs() < EPS
                };
                if (phase == 0 || !same(sources[phase - 1])) && (phase != 63 || !same(sources[0])) {
                    sources.push(source);
                    break;
                }
            }
        }
        Ok(sources)
    }

    /// Colored spans on one rail. Sources propagate in both directions, split
    /// at every reached junction, and merge without leaving uncolored gaps.
    pub fn spans(&self, wave: &Wave, index: usize, traveled: f64) -> ([(f64, f64); 3], usize) {
        let edge = &self.edges[index];
        let mut spans = [(0.0, 0.0); 3];
        let mut count = 0;
        let mut add = |a: f64, b: f64| {
            let (a, b) = (a.max(0.0), b.min(edge.length));
            if b > a {
                spans[count] = (a, b);
                count += 1;
            }
        };
        add(0.0, traveled - wave.distances[edge.a]);
        add(
            edge.length - (traveled - wave.distances[edge.b]),
            edge.length,
        );
        if index == wave.source.edge {
            add(wave.source.offset - traveled, wave.source.offset + traveled);
        }
        spans[..count].sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged = 0;
        for i in 0..count {
            if merged > 0 && spans[i].0 <= spans[merged - 1].1 + EPS {
                spans[merged - 1].1 = spans[merged - 1].1.max(spans[i].1);
            } else {
                spans[merged] = spans[i];
                merged += 1;
            }
        }
        (spans, merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn branch(degree: usize) -> Graph {
        let mut paths = vec![
            vec![[0.1, 0.5], [0.5, 0.5], [0.9, 0.5]],
            vec![[0.5, 0.5], [0.5, 0.9]],
        ];
        if degree == 4 {
            paths.push(vec![[0.5, 0.5], [0.5, 0.1]]);
        }
        Graph::new(&paths).unwrap()
    }
    #[test]
    fn splits_at_two_and_three_outgoing_branches() {
        for degree in [3, 4] {
            let graph = branch(degree);
            let first = graph
                .edges
                .iter()
                .position(|e| graph.nodes[e.a] == [0.1, 0.5])
                .unwrap();
            let wave = graph.wave(Source {
                edge: first,
                offset: 0.2,
            });
            let junction = graph.nodes.iter().position(|&p| p == [0.5, 0.5]).unwrap();
            assert_eq!(graph.adjacency[junction].len(), degree);
            for (index, edge) in graph.edges.iter().enumerate() {
                if index == first {
                    continue;
                }
                let before = graph.spans(&wave, index, 0.19);
                assert_eq!(before.1, 0);
                let after = graph.spans(&wave, index, 0.25);
                assert_eq!(after.1, 1);
                assert!((after.0[0].1 - after.0[0].0 - 0.05).abs() < EPS);
                assert!(edge.a == junction || edge.b == junction);
            }
        }
    }
    #[test]
    fn interior_sources_split_both_ways_and_never_start_on_nodes() {
        let graph = branch(4);
        for source in graph.sources(&mut Rng::seeded(7), 0.068).unwrap() {
            assert!(source.offset >= 0.068);
            assert!(graph.edges[source.edge].length - source.offset >= 0.068 - EPS);
            let wave = graph.wave(source);
            let (spans, count) = graph.spans(&wave, source.edge, 0.01);
            assert_eq!(count, 1);
            assert!((spans[0].0 - (source.offset - 0.01)).abs() < EPS);
            assert!((spans[0].1 - (source.offset + 0.01)).abs() < EPS);
        }
    }
    #[test]
    fn fronts_meet_on_loops_and_cover_every_rail() {
        let graph = Graph::new(&[vec![
            [0.1, 0.1],
            [0.9, 0.1],
            [0.9, 0.9],
            [0.1, 0.9],
            [0.1, 0.1],
        ]])
        .unwrap();
        let wave = graph.wave(Source {
            edge: 0,
            offset: 0.4,
        });
        for (index, edge) in graph.edges.iter().enumerate() {
            let (spans, count) = graph.spans(&wave, index, wave.reach);
            assert_eq!(count, 1);
            assert!(spans[0].0.abs() < EPS && (spans[0].1 - edge.length).abs() < EPS);
        }
    }
    #[test]
    fn rejects_disconnected_graphs() {
        assert!(Graph::new(&[vec![[0.1, 0.1], [0.2, 0.1]], vec![[0.8, 0.8], [0.9, 0.8]]]).is_err());
    }
}
