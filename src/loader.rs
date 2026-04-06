use artisan_core::{CharacterGraph, Entity};

use crate::error::HerolabError;

pub struct HerolabLoader;

impl HerolabLoader {
    pub fn parse_user_entities(_input: &str) -> Result<Vec<Entity>, HerolabError> {
        Ok(Vec::new())
    }

    pub fn parse_portfolio_graph(_input: &[u8]) -> Result<CharacterGraph, HerolabError> {
        Ok(CharacterGraph {
            nodes: Default::default(),
            edges: Vec::new(),
            metadata: artisan_core::domain::GraphMetadata {
                name: None,
                notes: Vec::new(),
            },
            opaque_extensions: Vec::new(),
        })
    }

    pub fn unparse_user_entities(_entities: &[Entity]) -> Result<String, HerolabError> {
        Ok(String::new())
    }
}
