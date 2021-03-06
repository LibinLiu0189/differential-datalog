/* Functions and transformers for use in graph processing */

use differential_dataflow::algorithms::graphs::propagate;
use differential_dataflow::algorithms::graphs::scc;
use differential_dataflow::collection::Collection;
use differential_dataflow::operators::consolidate::Consolidate;
use differential_dataflow::lattice::Lattice;
use timely::dataflow::scopes::Scope;
use std::mem;

pub fn graph_SCC<S,V,E,N,EF,LF>(edges: &Collection<S,V,Weight>, _edges: EF,
                                from: fn(&E) -> N,
                                to:   fn(&E) -> N,
                                _scclabels: LF) -> (Collection<S,V,Weight>)
where
     S: Scope,
     S::Timestamp: Lattice + Ord,
     V: Val,
     N: Val,
     E: Val,
     EF: Fn(V) -> E + 'static,
     LF: Fn((N,N)) -> V + 'static
{
    let pairs = edges.map(move |v| {
        let e = _edges(v);
        (from(&e), to(&e))
    });

    /* Recursively trim nodes without incoming and outgoing edges */
    let trimmed = scc::trim(&scc::trim(&pairs).map_in_place(|x| mem::swap(&mut x.0, &mut x.1)))
                  .map_in_place(|x| mem::swap(&mut x.0, &mut x.1));
    /* Edges that form cycles */
    let cycles = scc::strongly_connected(&trimmed);
    /* Initially each node is labeled by its own id */
    let nodes  = cycles.map_in_place(|x| x.0 = x.1.clone()).consolidate();
    /* Propagate smallest ID within SCC */
    let scclabels = propagate::propagate(&cycles, &nodes);
    scclabels.map(_scclabels)
}

pub fn graph_ConnectedComponents<S,V,E,N,EF,LF>(edges: &Collection<S,V,Weight>, _edges: EF,
                                                from: fn(&E) -> N,
                                                to:   fn(&E) -> N,
                                                _cclabels: LF) -> (Collection<S,V,Weight>)
where
     S: Scope,
     S::Timestamp: Lattice + Ord,
     V: Val,
     N: Val,
     E: Val,
     EF: Fn(V) -> E + 'static,
     LF: Fn((N,N)) -> V + 'static
{
    let pairs = edges.map(move |v| {
        let e = _edges(v);
        (from(&e), to(&e))
    });

    /* Initially each node is labeled by its own id */
    let nodes  = pairs.map_in_place(|x| x.0 = x.1.clone()).consolidate();
    let labels = propagate::propagate(&pairs, &nodes);
    labels.map(_cclabels)
}
