use ra_db::SyntaxDatabase;
use ra_syntax::{
    SyntaxNodeRef, AstNode,
    ast, algo::find_covering_node,
};

use crate::{
    TextRange, FileRange,
    db::RootDatabase,
};

pub(crate) fn extend_selection(db: &RootDatabase, frange: FileRange) -> TextRange {
    let source_file = db.source_file(frange.file_id);
    if let Some(macro_call) = find_macro_call(source_file.syntax(), frange.range) {
        if let Some(exp) = crate::macros::expand(db, frange.file_id, macro_call) {
            if let Some(dst_range) = exp.map_range_forward(frange.range) {
                if let Some(dst_range) = ra_editor::extend_selection(exp.source_file(), dst_range) {
                    if let Some(src_range) = exp.map_range_back(dst_range) {
                        return src_range;
                    }
                }
            }
        }
    }
    ra_editor::extend_selection(&source_file, frange.range).unwrap_or(frange.range)
}

fn find_macro_call(node: SyntaxNodeRef, range: TextRange) -> Option<ast::MacroCall> {
    find_covering_node(node, range)
        .ancestors()
        .find_map(ast::MacroCall::cast)
}

#[cfg(test)]
mod tests {
    use crate::mock_analysis::single_file_with_range;
    use test_utils::assert_eq_dbg;

    #[test]
    fn extend_selection_inside_macros() {
        let (analysis, frange) = single_file_with_range(
            "
            fn main() {
                ctry!(foo(|x| <|>x<|>));
            }
        ",
        );
        let r = analysis.extend_selection(frange);
        assert_eq_dbg("[51; 56)", &r);
    }
}
