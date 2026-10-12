//! ArkTS / TypeScript 着色规则（tree-sitter-typescript TSX 语法）
//!
//! @author sky
//! @author ljlVink

use super::TokenKind;

/// 返回 Some 表示命中着色，None 表示继续深入子节点
pub fn classify(node: &tree_sitter::Node, source: &[u8]) -> Option<TokenKind> {
    match node.kind() {
        "abstract" | "as" | "async" | "await" | "break" | "case" | "catch" | "class" | "const"
        | "continue" | "declare" | "default" | "delete" | "do" | "else" | "enum" | "export"
        | "extends" | "finally" | "for" | "from" | "function" | "get" | "if" | "implements"
        | "import" | "in" | "instanceof" | "interface" | "is" | "keyof" | "let" | "new" | "of"
        | "private" | "protected" | "public" | "readonly" | "return" | "satisfies" | "set"
        | "static" | "super" | "switch" | "this" | "throw" | "try" | "type" | "typeof" | "var"
        | "void" | "while" | "yield" | "namespace" | "module" | "asserts" | "infer"
        | "override" | "undefined" | "null" | "true" | "false" => Some(TokenKind::Keyword),
        "string"
        | "string_fragment"
        | "template_string"
        | "template_string_type"
        | "regex"
        | "regex_pattern" => Some(TokenKind::String),
        "number" | "shorthand_property_identifier_pattern" => Some(TokenKind::Number),
        "comment" => Some(TokenKind::Comment),
        "decorator" => Some(TokenKind::Annotation),
        "type_identifier" => Some(TokenKind::Type),
        "property_identifier" | "shorthand_property_identifier" => None,
        "identifier" => classify_identifier(node, source),
        _ => None,
    }
}

fn classify_identifier(node: &tree_sitter::Node, source: &[u8]) -> Option<TokenKind> {
    // 大写开头近似类型引用
    if let Some(text) = source.get(node.start_byte()..node.end_byte())
        && let Some(first) = text.first()
        && first.is_ascii_uppercase()
    {
        return Some(TokenKind::Type);
    }
    None
}
