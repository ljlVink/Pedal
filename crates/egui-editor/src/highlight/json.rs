//! JSON 着色规则
//!
//! @author sky

use super::TokenKind;

pub fn classify(node: &tree_sitter::Node, _source: &[u8]) -> Option<TokenKind> {
    match node.kind() {
        "string" | "string_content" | "\"" => {
            // pair 的 key 字段着 Type 色，其余着 String
            if let Some(parent) = node.parent()
                && parent.kind() == "pair"
                && parent
                    .child_by_field_name("key")
                    .is_some_and(|n| n.id() == node.id())
            {
                return Some(TokenKind::Type);
            }
            Some(TokenKind::String)
        }
        "number" => Some(TokenKind::Number),
        "true" | "false" | "null" => Some(TokenKind::Keyword),
        "comment" => Some(TokenKind::Comment),
        _ => None,
    }
}
