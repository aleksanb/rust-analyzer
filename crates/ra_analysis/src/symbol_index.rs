use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use fst::{self, Streamer};
use ra_editor::{self, FileSymbol};
use ra_syntax::{
    SourceFileNode,
    SyntaxKind::{self, *},
};
use ra_db::{SyntaxDatabase, SourceRootId};
use rayon::prelude::*;

use crate::{
    Cancelable,
    FileId, Query,
};

salsa::query_group! {
    pub(crate) trait SymbolsDatabase: SyntaxDatabase {
        fn file_symbols(file_id: FileId) -> Cancelable<Arc<SymbolIndex>> {
            type FileSymbolsQuery;
        }
        fn library_symbols(id: SourceRootId) -> Arc<SymbolIndex> {
            type LibrarySymbolsQuery;
            storage input;
        }
    }
}

fn file_symbols(db: &impl SyntaxDatabase, file_id: FileId) -> Cancelable<Arc<SymbolIndex>> {
    db.check_canceled()?;
    let syntax = db.source_file(file_id);
    Ok(Arc::new(SymbolIndex::for_file(file_id, syntax)))
}

#[derive(Default, Debug)]
pub(crate) struct SymbolIndex {
    symbols: Vec<(FileId, FileSymbol)>,
    map: fst::Map,
}

impl PartialEq for SymbolIndex {
    fn eq(&self, other: &SymbolIndex) -> bool {
        self.symbols == other.symbols
    }
}

impl Eq for SymbolIndex {}

impl Hash for SymbolIndex {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.symbols.hash(hasher)
    }
}

impl SymbolIndex {
    pub(crate) fn len(&self) -> usize {
        self.symbols.len()
    }

    pub(crate) fn for_files(
        files: impl ParallelIterator<Item = (FileId, SourceFileNode)>,
    ) -> SymbolIndex {
        let mut symbols = files
            .flat_map(|(file_id, file)| {
                ra_editor::file_symbols(&file)
                    .into_iter()
                    .map(move |symbol| (symbol.name.as_str().to_lowercase(), (file_id, symbol)))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        symbols.par_sort_by(|s1, s2| s1.0.cmp(&s2.0));
        symbols.dedup_by(|s1, s2| s1.0 == s2.0);
        let (names, symbols): (Vec<String>, Vec<(FileId, FileSymbol)>) =
            symbols.into_iter().unzip();
        let map = fst::Map::from_iter(names.into_iter().zip(0u64..)).unwrap();
        SymbolIndex { symbols, map }
    }

    pub(crate) fn for_file(file_id: FileId, file: SourceFileNode) -> SymbolIndex {
        SymbolIndex::for_files(rayon::iter::once((file_id, file)))
    }
}

impl Query {
    pub(crate) fn search(self, indices: &[Arc<SymbolIndex>]) -> Vec<(FileId, FileSymbol)> {
        let mut op = fst::map::OpBuilder::new();
        for file_symbols in indices.iter() {
            let automaton = fst::automaton::Subsequence::new(&self.lowercased);
            op = op.add(file_symbols.map.search(automaton))
        }
        let mut stream = op.union();
        let mut res = Vec::new();
        while let Some((_, indexed_values)) = stream.next() {
            if res.len() >= self.limit {
                break;
            }
            for indexed_value in indexed_values {
                let file_symbols = &indices[indexed_value.index];
                let idx = indexed_value.value as usize;

                let (file_id, symbol) = &file_symbols.symbols[idx];
                if self.only_types && !is_type(symbol.kind) {
                    continue;
                }
                if self.exact && symbol.name != self.query {
                    continue;
                }
                res.push((*file_id, symbol.clone()));
            }
        }
        res
    }
}

fn is_type(kind: SyntaxKind) -> bool {
    match kind {
        STRUCT_DEF | ENUM_DEF | TRAIT_DEF | TYPE_DEF => true,
        _ => false,
    }
}
