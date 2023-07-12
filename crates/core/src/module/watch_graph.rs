use std::hash::Hash;

use hashbrown::{HashMap, HashSet};
use petgraph::{
  graph::{DefaultIx, NodeIndex},
  stable_graph::StableDiGraph,
  EdgeDirection,
};

use crate::error::{CompilationError, Result};

/// ```md
///     a
///    /
///  v_b
/// ```
#[derive(Debug, Hash, PartialEq, Eq)]
enum EdgeMode {
  /// a -> v_b
  RootImport,
  /// a <- v_b
  WatchImport,
}

impl EdgeMode {
  fn is_root_import(&self) -> bool {
    *self == Self::RootImport
  }
}

pub struct WatchGraph {
  g: StableDiGraph<String, EdgeMode>,
  id_index_map: HashMap<String, NodeIndex<DefaultIx>>,
}

impl WatchGraph {
  pub fn new() -> Self {
    Self {
      g: StableDiGraph::new(),
      id_index_map: HashMap::new(),
    }
  }

  pub fn add_node(&mut self, node: String) {
    if !self.has_module(&node) {
      let index = self.g.add_node(node.clone());
      self.id_index_map.insert(node, index);
    }
  }

  pub fn add_edge(&mut self, from: &String, to: &String) -> Result<()> {
    let from_index = self.id_index_map.get(from).ok_or_else(|| {
      CompilationError::GenericError(format!(
        r#"from node "{}" does not exist in the module graph when add edge"#,
        from
      ))
    })?;

    let to_index = self.id_index_map.get(to).ok_or_else(|| {
      CompilationError::GenericError(format!(
        r#"to node "{}" does not exist in the module graph when add edge"#,
        to
      ))
    })?;
    //         a                          h               c
    //       /   \                      /               /
    //     b      v_c                v_e              v_e
    //   /       /   \
    // v_d     v_e   v_f
    //           \    /
    //            v_g
    // e change 引起 a、f、c 更新
    // c change 引起 a
    // g change 引起 a 更新
    //
    // m  |  root
    // a -> []
    // b -> []
    // c -> [d]
    // d -> [b]
    // e -> [a,h,c]
    // f -> [a]
    // g -> [a]
    // h -> []
    let roots: Vec<String> = self
      .relation_roots(from)
      .into_iter()
      .map(|item| item.clone())
      .collect();

    let roots: Vec<String> = [roots, vec![from.clone()]].concat();

    for root in roots.into_iter() {
      let root_id = self.id_index_map.get(&root).unwrap();

      self
        .g
        .update_edge(*to_index, *root_id, EdgeMode::RootImport);
    }

    self
      .g
      .update_edge(*from_index, *to_index, EdgeMode::WatchImport);

    Ok(())
  }

  pub fn resources(&self) -> Vec<&String> {
    let mut res = HashSet::new();

    for node in self.g.edge_indices() {
      match self.g.edge_weight(node).unwrap() {
        EdgeMode::WatchImport => {
          if let Some((_root, to)) = self.g.edge_endpoints(node) {
            res.insert(self.g.node_weight(to).unwrap());
          }
        }
        _ => {}
      };
    }

    res.into_iter().collect()
  }

  pub fn relation_roots(&self, dep: &String) -> Vec<&String> {
    let mut result = HashSet::new();

    if let Some(index) = self.id_index_map.get(dep) {
      let mut edges = self
        .g
        .neighbors_directed(*index, EdgeDirection::Outgoing)
        .detach();

      while let Some((edge, node)) = edges.next(&self.g) {
        if !self.g.edge_weight(edge).is_some_and(|e| e.is_root_import()) {
          continue;
        };

        result.insert(self.g.node_weight(node).unwrap());
      }
    };

    result.into_iter().collect()
  }

  pub fn has_module(&self, module_id: &String) -> bool {
    self.id_index_map.contains_key(module_id)
  }
}

#[cfg(test)]
mod tests {

  use super::WatchGraph;

  fn create_watch_graph_instance() -> WatchGraph {
    //          a
    //            \
    //            v_c
    //            /
    //          v_d
    let mut watch_graph = WatchGraph::new();
    watch_graph.add_node("a".into());
    let v_c: String = "v_c".into();
    let v_d: String = "v_d".into();
    watch_graph.add_node(v_c.clone());
    watch_graph.add_node(v_d.clone());

    watch_graph.add_edge(&"a".into(), &v_c).unwrap();
    watch_graph.add_edge(&v_c, &v_d).unwrap();

    watch_graph
  }

  #[test]
  fn dependencies() {
    //          a
    //            \
    //            v_c
    //            /
    //          v_d

    let v_c = "v_c".into();
    let v_d = "v_d".into();
    let watch_graph = create_watch_graph_instance();

    let expect = vec![&v_d, &v_c];
    for item in watch_graph.resources() {
      assert!(expect.contains(&item));
    }
  }

  #[test]
  fn relation_roots() {
    //          a
    //            \
    //            v_c
    //            /
    //          v_d
    let watch_graph = create_watch_graph_instance();

    assert_eq!(watch_graph.relation_roots(&"v_c".into()), vec![&"a"]);

    let mut r = watch_graph.relation_roots(&"v_d".into());
    r.sort();
    assert_eq!(r, [&"a".to_string(), &"v_c".to_string()])
  }
}
