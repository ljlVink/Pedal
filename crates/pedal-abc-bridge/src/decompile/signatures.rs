//! 函数签名类型：保留字节码中的类型，框架推断单独标注来源。
//! @author ljlVink

use crate::file::{AbcFile, BasicType, MethodInfo, Type};

/// proto 参数不包含隐式接收者，动态 ABI 的三个隐藏槽也不属于源参数。
pub(super) fn metadata(
    file: &AbcFile,
    method: &MethodInfo,
) -> Option<(Vec<Option<String>>, Option<String>)> {
    if method.proto_off == 0 {
        return None;
    }
    let proto = file.read_proto(method.proto_off).ok()?;
    Some((
        proto.params.iter().map(|ty| type_name(file, *ty)).collect(),
        type_name(file, proto.ret),
    ))
}

fn type_name(file: &AbcFile, ty: Type) -> Option<String> {
    Some(match ty {
        Type::Basic(BasicType::Any) => return None,
        Type::Basic(BasicType::Void) => "void".into(),
        Type::Basic(BasicType::U1) => "boolean".into(),
        Type::Basic(BasicType::I8) => "byte".into(),
        Type::Basic(BasicType::U8) => "ubyte".into(),
        Type::Basic(BasicType::I16) => "short".into(),
        Type::Basic(BasicType::U16) => "ushort".into(),
        Type::Basic(BasicType::I32) => "int".into(),
        Type::Basic(BasicType::U32) => "uint".into(),
        Type::Basic(BasicType::F32) => "float".into(),
        Type::Basic(BasicType::F64) => "double".into(),
        Type::Basic(BasicType::I64) => "long".into(),
        Type::Basic(BasicType::U64) => "ulong".into(),
        Type::Ref(off) => debug_type(&file.class_name(off).ok()?)?,
    })
}

/// 只接受已知描述符/类型名；动态运行时统一对象类型不等于原始源码类型。
pub(super) fn debug_type(raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw.len() > 1024
        || matches!(
            raw,
            "any" | "A" | "L_ECMAScript_OBJECT;" | "_ECMAScript_OBJECT"
        )
    {
        return None;
    }
    if let Some((name, rest)) = descriptor(raw, 0)
        && rest.is_empty()
    {
        return Some(name);
    }
    let primitive = match raw {
        "u1" => "boolean",
        "i8" => "byte",
        "u8" => "ubyte",
        "i16" => "short",
        "u16" => "ushort",
        "i32" => "int",
        "u32" => "uint",
        "i64" => "long",
        "u64" => "ulong",
        "f32" => "float",
        "f64" => "double",
        other => other,
    };
    valid_type_path(primitive).then(|| primitive.to_string())
}

fn valid_type_path(name: &str) -> bool {
    name.split('.').all(|part| {
        let mut chars = part.chars();
        chars
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
            && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
    })
}

fn descriptor(raw: &str, depth: usize) -> Option<(String, &str)> {
    if depth > 32 {
        return None;
    }
    if let Some(rest) = raw.strip_prefix('[') {
        let (inner, tail) = descriptor(rest, depth + 1)?;
        return Some((format!("{inner}[]"), tail));
    }
    if let Some(mut rest) = raw.strip_prefix("{U") {
        let mut members = Vec::new();
        while !rest.starts_with('}') {
            let (member, tail) = descriptor(rest, depth + 1)?;
            members.push(member);
            rest = tail;
        }
        return (members.len() >= 2).then(|| (format!("({})", members.join(" | ")), &rest[1..]));
    }
    if let Some(rest) = raw.strip_prefix('L') {
        let (name, tail) = rest.split_once(';')?;
        let name = name.strip_prefix('@').unwrap_or(name).replace('/', ".");
        if !valid_type_path(&name) || name == "_ECMAScript_OBJECT" {
            return None;
        }
        let name = match name.as_str() {
            "std.core.Null" => "null".into(),
            "std.core.Undefined" => "undefined".into(),
            "std.core.String" => "string".into(),
            _ => name,
        };
        return Some((name, tail));
    }
    let (first, tail) = raw.split_at_checked(1)?;
    let name = match first {
        "V" => "void",
        "Z" => "boolean",
        "B" => "byte",
        "S" => "short",
        "C" => "char",
        "I" => "int",
        "J" => "long",
        "F" => "float",
        "D" => "double",
        _ => return None,
    };
    Some((name.into(), tail))
}

/// ArkUI 生成构造器的参数角色；只由已确认的 ViewPU 类和构造器数据流触发。
pub(super) const ARKUI_PARAMETERS: [(&str, &str); 6] = [
    ("parent", "IView | undefined"),
    ("params", "unknown"),
    ("localStorage", "LocalStorage | undefined"),
    ("elmtId", "number | undefined"),
    ("paramsLambda", "(() => Object) | undefined"),
    ("extraInfo", "ExtraInfo | undefined"),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptors_preserve_arrays_unions_and_runtime_unknowns() {
        assert_eq!(debug_type("[[I").as_deref(), Some("int[][]"));
        assert_eq!(
            debug_type("[{UL@ohos/base/BusinessError;Lstd/core/Null;}").as_deref(),
            Some("(ohos.base.BusinessError | null)[]")
        );
        assert_eq!(debug_type("Probe_Params").as_deref(), Some("Probe_Params"));
        for raw in [
            "any",
            "L_ECMAScript_OBJECT;",
            "LBad;garbage",
            "bad; statement",
            "{ULstd/core/Null;}",
        ] {
            assert!(debug_type(raw).is_none(), "{raw}");
        }
    }
}
