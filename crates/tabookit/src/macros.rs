//! 宏工具。

/// 在闭包内启用 `?` 操作符，返回 `Option<T>`。
/// 用于在非 Option 返回值的函数中写 `?` 链。
///
/// 单表达式形式：
/// ```ignore
/// let ip = chain!(=> headers.get("xff")?.to_str().ok()?.split(',').next()?);
/// ```
///
/// 块形式（最后一个表达式自动包 `Some`）：
/// ```ignore
/// let ip = chain! {
///     let s = headers.get("xff")?.to_str().ok()?;
///     s.split(',').next()?.trim().to_owned()
/// };
/// ```
#[macro_export]
macro_rules! chain {
    (=> $($body:tt)*) => {
        (|| -> Option<_> { Some($($body)*) })()
    };
    { $($body:tt)* } => {
        (|| -> Option<_> { Some({ $($body)* }) })()
    };
}

/// Kotlin `?:` 操作符。从 `Option` 中取值，`None` 时用 fallback。
///
/// ```ignore
/// let name = or!(opt_name, "default".to_owned());
/// ```
#[macro_export]
macro_rules! or {
    ($opt:expr, $fallback:expr $(,)?) => {
        match $opt {
            Some(v) => v,
            None => $fallback,
        }
    };
}

/// 从多个 `Option` 中取第一个非空值。按顺序尝试，命中后用 `=> expr` 变换。
///
/// ```ignore
/// first!(it in
///     self.last_mismatch => &it.1,
///     self.last_thinking_err => &it.1,
///     self.last_retry_body => it,
///     else "",
/// )
/// ```
#[macro_export]
macro_rules! first {
    ($name:ident in else $default:expr $(,)?) => { $default };
    ($name:ident in $opt:expr => $body:expr, $($rest:tt)*) => {
        match &$opt {
            Some($name) => $body,
            _ => $crate::first!($name in $($rest)*)
        }
    };
}
///
/// ```ignore
/// let val = chain! {
///     let s = get_str()?;
///     ensure!(!s.is_empty());
///     s.to_owned()
/// };
/// ```
#[macro_export]
macro_rules! ensure {
    ($cond:expr) => {
        if !($cond) {
            None?
        }
    };
}

/// 将 struct 定义与 impl 块合并为 Kotlin 风格的类定义。
///
/// struct 块之后的所有内容自动放入 `impl` 块。
/// 支持泛型参数（含 bounds），宏会自动剥离 bounds 生成正确的 impl 签名。
///
/// 无泛型：
/// ```ignore
/// class! {
///     #[derive(Debug)]
///     pub struct Foo {
///         /// X 坐标
///         pub x: i32,
///         y: String,
///     }
///
///     pub fn new(x: i32) -> Self {
///         Self { x, y: String::new() }
///     }
///
///     fn y(&self) -> &str {
///         &self.y
///     }
/// }
/// ```
///
/// 带泛型（bounds 自动剥离）：
/// ```ignore
/// class! {
///     pub struct Wrapper<T: Clone + Send, U> {
///         inner: T,
///         tag: U,
///     }
///
///     pub fn into_inner(self) -> T {
///         self.inner
///     }
/// }
/// // 展开为:
/// // pub struct Wrapper<T: Clone + Send, U> { ... }
/// // impl<T: Clone + Send, U> Wrapper<T, U> { ... }
/// ```
#[macro_export]
macro_rules! class {
    // 无泛型
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident {
            $($fields:tt)*
        }
        $($impl_body:tt)*
    ) => {
        $(#[$attr])*
        $vis struct $name {
            $($fields)*
        }
        impl $name {
            $($impl_body)*
        }
    };

    // 有泛型 - 进入 TT muncher 提取 <> 内容
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident < $($rest:tt)*
    ) => {
        $crate::class! { @extract
            attrs = [$(#[$attr])*],
            vis = [$vis],
            name = [$name],
            gen = [],
            depth = [],
            rest = [$($rest)*],
        }
    };

    // 提取泛型参数
    // TT muncher 逐 token 扫描 <> 之间的内容，用 depth 栈跟踪嵌套深度。

    // `>>` depth 1 → 同时关闭内层和外层
    (@extract
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        gen = [$($g:tt)*], depth = [+],
        rest = [>> $($r:tt)*],
    ) => {
        $crate::class! { @parsed
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)* >], rest = [$($r)*],
        }
    };

    // `>>` depth 2+ → 关闭两层嵌套
    (@extract
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        gen = [$($g:tt)*], depth = [+ + $($d:tt)*],
        rest = [>> $($r:tt)*],
    ) => {
        $crate::class! { @extract
            attrs = [$($a)*], vis = [$v], name = [$n],
            gen = [$($g)* >>], depth = [$($d)*], rest = [$($r)*],
        }
    };

    // `>` depth 0 → 外层闭合，提取完成
    (@extract
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        gen = [$($g:tt)*], depth = [],
        rest = [> $($r:tt)*],
    ) => {
        $crate::class! { @parsed
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], rest = [$($r)*],
        }
    };

    // `>` depth > 0 → 关闭一层嵌套
    (@extract
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        gen = [$($g:tt)*], depth = [+ $($d:tt)*],
        rest = [> $($r:tt)*],
    ) => {
        $crate::class! { @extract
            attrs = [$($a)*], vis = [$v], name = [$n],
            gen = [$($g)* >], depth = [$($d)*], rest = [$($r)*],
        }
    };

    // `<` → 打开一层嵌套
    (@extract
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        gen = [$($g:tt)*], depth = [$($d:tt)*],
        rest = [< $($r:tt)*],
    ) => {
        $crate::class! { @extract
            attrs = [$($a)*], vis = [$v], name = [$n],
            gen = [$($g)* <], depth = [+ $($d)*], rest = [$($r)*],
        }
    };

    // 其他 token → 收入缓冲
    (@extract
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        gen = [$($g:tt)*], depth = [$($d:tt)*],
        rest = [$tok:tt $($r:tt)*],
    ) => {
        $crate::class! { @extract
            attrs = [$($a)*], vis = [$v], name = [$n],
            gen = [$($g)* $tok], depth = [$($d)*], rest = [$($r)*],
        }
    };

    // 泛型提取完毕，分离 fields 和 impl body，进入 bounds 剥离阶段
    (@parsed
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*],
        rest = [{ $($fields:tt)* } $($impl_body:tt)*],
    ) => {
        $crate::class! { @strip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($fields)*], impl_body = [$($impl_body)*],
            use_params = [], depth = [], rest = [$($g)*],
        }
    };

    // 剥离 bounds
    // 遍历逗号分隔的泛型参数，只提取参数名（不含 bounds）用于 impl 的类型位置。
    // <T: Clone, U: Send> → impl<T: Clone, U: Send> Foo<T, U>

    // 结束 → 输出 struct + impl
    (@strip
        attrs = [$(#[$attr:meta])*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [], rest = [],
    ) => {
        $(#[$attr])*
        $v struct $n<$($g)*> { $($f)* }
        impl<$($g)*> $n<$($u)*> { $($ib)* }
    };

    // lifetime 参数
    (@strip
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [], rest = [$lt:lifetime $($r:tt)*],
    ) => {
        $crate::class! { @skip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($f)*], impl_body = [$($ib)*],
            use_params = [$($u)* $lt,], depth = [], rest = [$($r)*],
        }
    };

    // const 参数
    (@strip
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [], rest = [const $p:ident $($r:tt)*],
    ) => {
        $crate::class! { @skip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($f)*], impl_body = [$($ib)*],
            use_params = [$($u)* $p,], depth = [], rest = [$($r)*],
        }
    };

    // type 参数
    (@strip
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [], rest = [$p:ident $($r:tt)*],
    ) => {
        $crate::class! { @skip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($f)*], impl_body = [$($ib)*],
            use_params = [$($u)* $p,], depth = [], rest = [$($r)*],
        }
    };

    // 跳过 bound tokens
    // 消耗单个参数的 bound 部分（: Clone + Send 等），直到遇到 `,` 或结束。

    // `,` depth 0 → 下一个参数
    (@skip
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [],
        rest = [, $($r:tt)*],
    ) => {
        $crate::class! { @strip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($f)*], impl_body = [$($ib)*],
            use_params = [$($u)*], depth = [], rest = [$($r)*],
        }
    };

    // 空 depth 0 → 结束
    (@skip
        attrs = [$(#[$attr:meta])*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [], rest = [],
    ) => {
        $(#[$attr])*
        $v struct $n<$($g)*> { $($f)* }
        impl<$($g)*> $n<$($u)*> { $($ib)* }
    };

    // `>>` depth 1
    (@skip
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [+],
        rest = [>> $($r:tt)*],
    ) => {
        $crate::class! { @skip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($f)*], impl_body = [$($ib)*],
            use_params = [$($u)*], depth = [], rest = [$($r)*],
        }
    };

    // `>>` depth 2+
    (@skip
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [+ + $($d:tt)*],
        rest = [>> $($r:tt)*],
    ) => {
        $crate::class! { @skip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($f)*], impl_body = [$($ib)*],
            use_params = [$($u)*], depth = [$($d)*], rest = [$($r)*],
        }
    };

    // `<` → 增加深度
    (@skip
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [$($d:tt)*],
        rest = [< $($r:tt)*],
    ) => {
        $crate::class! { @skip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($f)*], impl_body = [$($ib)*],
            use_params = [$($u)*], depth = [+ $($d)*], rest = [$($r)*],
        }
    };

    // `>` depth > 0 → 减少深度
    (@skip
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [+ $($d:tt)*],
        rest = [> $($r:tt)*],
    ) => {
        $crate::class! { @skip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($f)*], impl_body = [$($ib)*],
            use_params = [$($u)*], depth = [$($d)*], rest = [$($r)*],
        }
    };

    // 其他 token → 跳过
    (@skip
        attrs = [$($a:tt)*], vis = [$v:vis], name = [$n:ident],
        generics = [$($g:tt)*], fields = [$($f:tt)*], impl_body = [$($ib:tt)*],
        use_params = [$($u:tt)*], depth = [$($d:tt)*],
        rest = [$_:tt $($r:tt)*],
    ) => {
        $crate::class! { @skip
            attrs = [$($a)*], vis = [$v], name = [$n],
            generics = [$($g)*], fields = [$($f)*], impl_body = [$($ib)*],
            use_params = [$($u)*], depth = [$($d)*], rest = [$($r)*],
        }
    };
}
