//! Automatic routers: every pair of a router's networks may reach each other, both ways.

use isoloom_core::Reach;

use crate::draft::Draft;

/// The automatic routers' `reach` rules, each pair once.
pub fn add_reach_rules(draft: &mut Draft) {
    for nets in draft.routers.values() {
        for from in nets {
            for to in nets {
                let known = draft.reach.iter().any(|r| &r.from == from && &r.to == to);
                if from != to && !known {
                    draft.reach.push(Reach {
                        from: from.clone(),
                        to: to.clone(),
                        ports: vec![],
                    });
                }
            }
        }
    }
}
