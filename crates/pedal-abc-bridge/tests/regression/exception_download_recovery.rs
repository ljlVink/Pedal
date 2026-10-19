//! 下载业务异常区域出口的合成字节码与执行语义回归。
//! @author ljlVink

use pedal_abc_bridge::decompile::lift::{Expr, Stmt};
use pedal_abc_bridge::file::{
    AbcFile, CatchBlock, ClassInfo, CodeInfo, FileHeader, FunctionKind, HeaderKind, MethodInfo,
    TryBlock,
};
use pedal_abc_bridge::{decompile, disasm};
use std::collections::HashMap;

#[derive(Default)]
struct Asm {
    bytes: Vec<u8>,
    labels: HashMap<&'static str, usize>,
    jumps: Vec<(usize, &'static str)>,
}

impl Asm {
    fn mark(&mut self, label: &'static str) {
        self.labels.insert(label, self.bytes.len());
    }
    fn emit(&mut self, bytes: &[u8]) {
        self.bytes.extend(bytes);
    }
    fn jump(&mut self, opcode: u8, label: &'static str) {
        self.jumps.push((self.bytes.len(), label));
        self.emit(&[opcode, 0]);
    }
    fn int(&mut self, value: i32) {
        self.emit(&[0x62]);
        self.emit(&value.to_le_bytes());
    }
    fn finish(
        mut self,
        start: &'static str,
        end: &'static str,
        handler: &'static str,
    ) -> (Vec<u8>, Vec<TryBlock>) {
        for (pc, label) in self.jumps {
            self.bytes[pc + 1] =
                i8::try_from(self.labels[label] as isize - pc as isize).unwrap() as u8;
        }
        let region = TryBlock {
            start_pc: self.labels[start] as u32,
            length: (self.labels[end] - self.labels[start]) as u32,
            catch_blocks: vec![CatchBlock {
                type_idx: 0,
                handler_pc: self.labels[handler] as u32,
                code_size: 0,
            }],
        };
        (self.bytes, vec![region])
    }
}

fn fixture(bytes: &[u8], regions: &[TryBlock], vregs: u32) -> (AbcFile, ClassInfo, MethodInfo) {
    let method = MethodInfo {
        offset: 0,
        class_off: 0,
        proto_off: 0,
        name: "probe".into(),
        header_index: 0,
        kind: FunctionKind::Function,
        debug_info_off: 0,
        foreign: false,
        code: Some(CodeInfo {
            offset: 0,
            num_vregs: vregs,
            num_args: 8,
            code_size: bytes.len() as u32,
            ins_off: 0,
            try_blocks: regions.to_vec(),
        }),
    };
    let class = ClassInfo {
        offset: 0,
        name: "Probe".into(),
        access_flags: 0,
        source_file: None,
        fields: vec![],
        methods: vec![],
        module_record_idx: None,
    };
    let file = AbcFile {
        data: bytes.to_vec(),
        class_index: vec![],
        index_headers: vec![],
        classes: vec![],
        header: FileHeader {
            version: [12, 0, 0, 0],
            file_size: bytes.len() as u32,
            foreign_off: 0,
            foreign_size: 0,
            num_classes: 0,
            class_idx_off: 0,
            num_lnps: 0,
            lnp_idx_off: 0,
            num_literalarrays: 0,
            literalarray_idx_off: 0,
            num_index_regions: 0,
            index_section_off: 0,
            kind: HeaderKind::Modern,
        },
    };
    (file, class, method)
}

fn lifted(bytes: &[u8], regions: &[TryBlock], vregs: u32) -> Vec<Stmt> {
    let (file, class, method) = fixture(bytes, regions, vregs);
    let code = method.code.as_ref().unwrap();
    let ins = disasm::decode(bytes).unwrap();
    let cfg = decompile::cfg::build(&ins, bytes.len(), regions);
    let mut body = decompile::lift::lift(&file, &class, &method, code, &cfg, None).stmts;
    decompile::structure::polish(&mut body);
    body
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Undefined,
    Hole,
    Object,
    String(String),
    Int(i64),
    Bool(bool),
    Function(usize),
}
impl Value {
    fn truthy(&self) -> bool {
        !matches!(self, Self::Undefined | Self::Int(0) | Self::Bool(false))
    }
}
#[derive(Clone, Debug, PartialEq)]
enum Completion {
    Normal,
    Return(Value),
    Throw(Value),
    Break,
    Continue,
}
struct Execution {
    args: Vec<Value>,
    regs: HashMap<u32, Value>,
    errors: HashMap<String, Value>,
    calls: Vec<usize>,
    throwing: Option<usize>,
    thrown_value: Option<Value>,
    loop_callback: bool,
}
impl Execution {
    fn new(flag: bool, throwing: Option<usize>) -> Self {
        Self {
            args: vec![
                Value::Undefined,
                Value::Undefined,
                Value::Undefined,
                Value::Bool(flag),
                Value::Function(0),
                Value::Function(1),
                Value::Function(2),
                Value::Function(3),
            ],
            regs: HashMap::new(),
            errors: HashMap::new(),
            calls: vec![],
            throwing,
            thrown_value: None,
            loop_callback: false,
        }
    }
    fn call(&mut self, value: Value) -> Result<Value, Value> {
        let Value::Function(id) = value else {
            panic!("non-callable {value:?}")
        };
        self.calls.push(id);
        if self.throwing == Some(id) {
            return Err(self
                .thrown_value
                .clone()
                .unwrap_or(Value::Int(-(id as i64) - 1)));
        }
        if self.loop_callback && id == 0 {
            return Ok(Value::Bool(
                self.calls.iter().filter(|v| **v == 0).count() < 3,
            ));
        }
        Ok(Value::Int(id as i64 + 10))
    }
    fn expr(&mut self, expr: &Expr) -> Result<Value, Value> {
        Ok(match expr {
            Expr::Arg(n) => self.args[*n as usize].clone(),
            Expr::Reg(n) => self.regs.get(n).cloned().unwrap_or(Value::Undefined),
            Expr::Name(n) => self.errors[n].clone(),
            Expr::Int(n) => Value::Int(*n),
            Expr::Bool(v) => Value::Bool(*v),
            Expr::Special("undefined") => Value::Undefined,
            Expr::Object(values) if values.is_empty() => Value::Object,
            Expr::Bin(op @ ("===" | "!==" | "==" | "!="), left, right) => {
                let left = self.expr(left)?;
                let right = self.expr(right)?;
                let equal = left == right
                    || (matches!(*op, "==" | "!=")
                        && matches!((&left, &right),
                    (Value::Object, Value::String(text)) | (Value::String(text), Value::Object) if text == "[object Object]"));
                Value::Bool(if matches!(*op, "!==" | "!=") {
                    !equal
                } else {
                    equal
                })
            }
            Expr::Bin("||", left, right) => {
                let left = self.expr(left)?;
                if left.truthy() {
                    left
                } else {
                    self.expr(right)?
                }
            }
            Expr::Bin("+", left, right) => {
                let (Value::Int(left), Value::Int(right)) = (self.expr(left)?, self.expr(right)?)
                else {
                    panic!("integer test addition")
                };
                Value::Int(left + right)
            }
            Expr::Un("!", x) => Value::Bool(!self.expr(x)?.truthy()),
            Expr::Assign(target, value) => {
                let value = self.expr(value)?;
                let Expr::Reg(n) = target.as_ref() else {
                    panic!("unsupported target {target:?}")
                };
                self.regs.insert(*n, value.clone());
                value
            }
            Expr::Cond(c, t, f) => {
                if self.expr(c)?.truthy() {
                    self.expr(t)?
                } else {
                    self.expr(f)?
                }
            }
            Expr::Call {
                callee,
                args,
                new: false,
                this_arg: None,
            } if args.is_empty() => {
                let callee = self.expr(callee)?;
                self.call(callee)?
            }
            other => panic!("unsupported test expression {other:?}"),
        })
    }
    fn stmts(&mut self, stmts: &[Stmt]) -> Completion {
        for stmt in stmts {
            let result = match stmt {
                Stmt::Expr(e) => match self.expr(e) {
                    Ok(_) => Completion::Normal,
                    Err(e) => Completion::Throw(e),
                },
                Stmt::Return(None) => Completion::Return(Value::Undefined),
                Stmt::Return(Some(e)) => match self.expr(e) {
                    Ok(v) => Completion::Return(v),
                    Err(e) => Completion::Throw(e),
                },
                Stmt::Throw(e) => Completion::Throw(self.expr(e).unwrap_or_else(|e| e)),
                Stmt::If(c, t, f) => match self.expr(c) {
                    Ok(v) => self.stmts(if v.truthy() { t } else { f }),
                    Err(e) => Completion::Throw(e),
                },
                Stmt::Try(body, catches) => {
                    let completion = self.stmts(body);
                    if let Completion::Throw(error) = completion {
                        let (name, catch) = catches.first().expect("test catch");
                        self.errors.insert(name.clone().unwrap(), error);
                        self.stmts(catch)
                    } else {
                        completion
                    }
                }
                Stmt::While(c, body) => {
                    let mut completion = Completion::Normal;
                    for iteration in 0..200 {
                        assert!(iteration < 199, "test loop did not terminate");
                        match self.expr(c) {
                            Ok(v) if !v.truthy() => break,
                            Err(e) => {
                                completion = Completion::Throw(e);
                                break;
                            }
                            _ => {}
                        }
                        match self.stmts(body) {
                            Completion::Normal | Completion::Continue => {}
                            Completion::Break => break,
                            other => {
                                completion = other;
                                break;
                            }
                        }
                    }
                    completion
                }
                Stmt::Break => Completion::Break,
                Stmt::Continue => Completion::Continue,
                other => panic!("unsupported test statement {other:?}"),
            };
            if result != Completion::Normal {
                return result;
            }
        }
        Completion::Normal
    }
    /// 独立执行原始字节码；异常由原异常表决定，不使用提升后的区域分析。
    fn bytecode(&mut self, bytes: &[u8], regions: &[TryBlock], vregs: u32) -> Completion {
        let ins = disasm::decode(bytes).unwrap();
        let mut pc = 0;
        let mut acc = Value::Undefined;
        for _ in 0..200 {
            let i = ins.iter().find(|i| i.pc == pc).unwrap();
            let mut next = pc + i.meta.size as usize;
            let read = |n: u64, execution: &Self| {
                if n < vregs as u64 {
                    execution
                        .regs
                        .get(&(n as u32))
                        .cloned()
                        .unwrap_or(Value::Undefined)
                } else {
                    execution.args[(n - vregs as u64) as usize].clone()
                }
            };
            let mut error = None;
            match i.meta.mnemonic {
                "lda" => acc = read(i.field("v").unwrap(), self),
                "sta" => {
                    self.regs.insert(i.field("v").unwrap() as u32, acc.clone());
                }
                "mov" => {
                    self.regs.insert(
                        i.field("v1").unwrap() as u32,
                        read(i.field("v2").unwrap(), self),
                    );
                }
                "ldai" => acc = Value::Int(i.field("imm").unwrap() as u32 as i32 as i64),
                "ldtrue" => acc = Value::Bool(true),
                "ldfalse" => acc = Value::Bool(false),
                "ldundefined" => acc = Value::Undefined,
                "ldhole" => acc = Value::Hole,
                "noteq" => acc = Value::Bool(read(i.field("v").unwrap(), self) != acc),
                "add2" => {
                    let (Value::Int(left), Value::Int(right)) =
                        (read(i.field("v").unwrap(), self), acc)
                    else {
                        panic!("integer bytecode addition")
                    };
                    acc = Value::Int(left + right);
                }
                "callarg0" => match self.call(acc.clone()) {
                    Ok(v) => acc = v,
                    Err(e) => error = Some(e),
                },
                "jmp" => next = i.jump_target().unwrap(),
                "jnez" if acc.truthy() => next = i.jump_target().unwrap(),
                "jeqz" if !acc.truthy() => next = i.jump_target().unwrap(),
                "jnez" | "jeqz" => {}
                "return" => return Completion::Return(acc),
                "returnundefined" => return Completion::Return(Value::Undefined),
                "throw" => error = Some(acc.clone()),
                other => panic!("unsupported test instruction {other}"),
            }
            if let Some(error) = error {
                if let Some(region) = regions
                    .iter()
                    .filter(|r| pc >= r.start_pc as usize && pc < (r.start_pc + r.length) as usize)
                    .min_by_key(|r| r.length)
                {
                    acc = error;
                    next = region.catch_blocks[0].handler_pc as usize;
                } else {
                    return Completion::Throw(error);
                }
            }
            pc = next;
        }
        panic!("test bytecode did not terminate")
    }
}

fn compare(
    bytes: &[u8],
    regions: &[TryBlock],
    vregs: u32,
    flag: bool,
    throwing: Option<usize>,
    loop_callback: bool,
) {
    let body = lifted(bytes, regions, vregs);
    let mut original = Execution::new(flag, throwing);
    original.loop_callback = loop_callback;
    let expected = original.bytecode(bytes, regions, vregs);
    let mut recovered = Execution::new(flag, throwing);
    recovered.loop_callback = loop_callback;
    let actual = recovered.stmts(&body);
    assert_eq!(
        (actual, recovered.calls),
        (expected, original.calls),
        "{body:#?}"
    );
}

#[test]
fn split_try_tail_retains_both_calls_and_catch_call_scope() {
    let mut a = Asm::default();
    a.mark("start");
    a.emit(&[0x60, 4]);
    a.jump(0x51, "then");
    a.emit(&[0x60, 5, 0x29, 0]);
    a.jump(0x4d, "join");
    a.mark("then");
    a.emit(&[0x60, 6, 0x29, 0]);
    a.mark("end");
    a.jump(0x4d, "join");
    a.mark("handler");
    a.emit(&[0x61, 0, 0x60, 7, 0x29, 0]);
    a.mark("join");
    a.emit(&[0x60, 8, 0x29, 0, 0x64]);
    let (bytes, regions) = a.finish("start", "end", "handler");
    for flag in [false, true] {
        for throwing in [None, Some(0), Some(1), Some(2), Some(3)] {
            compare(&bytes, &regions, 1, flag, throwing, false);
        }
    }
}

#[test]
fn pending_expression_before_try_keeps_its_original_exception_scope() {
    let mut a = Asm::default();
    a.emit(&[0x60, 5, 0x29, 0]);
    a.mark("start");
    a.emit(&[0x60, 6, 0x29, 0]);
    a.mark("end");
    a.jump(0x4d, "join");
    a.mark("handler");
    a.emit(&[0x61, 0, 0x60, 7, 0x29, 0]);
    a.mark("join");
    a.emit(&[0x65]);
    let (bytes, regions) = a.finish("start", "end", "handler");
    for throwing in [None, Some(0), Some(1), Some(2)] {
        compare(&bytes, &regions, 1, false, throwing, false);
    }
}

#[test]
fn early_return_in_try_skips_the_shared_continuation() {
    let mut a = Asm::default();
    a.mark("start");
    a.emit(&[0x60, 4]);
    a.jump(0x51, "early");
    a.emit(&[0x60, 5, 0x29, 0]);
    a.jump(0x4d, "join");
    a.mark("early");
    a.int(71);
    a.emit(&[0x64]);
    a.mark("end");
    a.jump(0x4d, "join");
    a.mark("handler");
    a.emit(&[0x61, 0, 0x60, 7, 0x29, 0]);
    a.mark("join");
    a.emit(&[0x60, 8, 0x29, 0, 0x64]);
    let (bytes, regions) = a.finish("start", "end", "handler");
    for flag in [false, true] {
        for throwing in [None, Some(0), Some(2), Some(3)] {
            compare(&bytes, &regions, 1, flag, throwing, false);
        }
    }
}

#[test]
fn normal_and_caught_accumulator_values_merge_before_shared_return() {
    let mut a = Asm::default();
    a.mark("start");
    a.emit(&[0x60, 5, 0x29, 0]);
    a.mark("end");
    a.jump(0x4d, "join");
    a.mark("handler");
    a.emit(&[0x61, 0]);
    a.int(33);
    a.mark("join");
    a.emit(&[0x64]);
    let (bytes, regions) = a.finish("start", "end", "handler");
    for throwing in [None, Some(0)] {
        compare(&bytes, &regions, 1, false, throwing, false);
    }
}

#[test]
fn register_copy_tails_merge_without_dropping_conditional_assignment() {
    let mut a = Asm::default();
    a.int(1);
    a.emit(&[0x61, 1]);
    a.mark("start");
    a.emit(&[0x60, 5]);
    a.jump(0x51, "join");
    a.int(9);
    a.emit(&[0x61, 0]);
    a.mark("end");
    a.jump(0x4d, "copy");
    a.mark("handler");
    a.emit(&[0x00, 0x61, 1]);
    a.jump(0x4d, "join");
    a.mark("copy");
    a.emit(&[0x45, 1, 0]);
    a.mark("join");
    a.emit(&[0x60, 1, 0x64]);
    let (bytes, regions) = a.finish("start", "end", "handler");
    for flag in [false, true] {
        compare(&bytes, &regions, 2, flag, None, false);
    }
}

#[test]
fn trampoline_back_into_protected_loop_remains_a_back_edge() {
    let mut a = Asm::default();
    a.mark("start");
    a.emit(&[0x60, 5, 0x29, 0]);
    a.jump(0x4f, "join");
    a.mark("end");
    a.jump(0x4d, "start");
    a.mark("handler");
    a.emit(&[0x61, 0, 0x60, 7, 0x29, 0, 0x65]);
    a.mark("join");
    a.emit(&[0x65]);
    let (bytes, regions) = a.finish("start", "end", "handler");
    for throwing in [None, Some(0), Some(2)] {
        compare(&bytes, &regions, 1, false, throwing, true);
    }
}

#[test]
fn effectful_distinct_tails_execute_outside_their_original_try() {
    let mut a = Asm::default();
    a.mark("start");
    a.emit(&[0x60, 4]);
    a.jump(0x51, "right");
    a.jump(0x4d, "left");
    a.mark("end");
    a.emit(&[0x65]);
    a.mark("handler");
    a.emit(&[0x65]);
    a.mark("left");
    a.emit(&[0x60, 5, 0x29, 0]);
    a.jump(0x4d, "join");
    a.mark("right");
    a.emit(&[0x60, 6, 0x29, 0]);
    a.mark("join");
    a.emit(&[0x64]);
    let (bytes, regions) = a.finish("start", "end", "handler");
    for flag in [false, true] {
        for throwing in [None, Some(0), Some(1)] {
            compare(&bytes, &regions, 1, flag, throwing, false);
        }
    }
}

#[test]
fn unsupported_instruction_does_not_become_an_executable_exception_state_machine() {
    let mut a = Asm::default();
    a.mark("start");
    a.emit(&[0xfb, 0x04, 0, 0, 0, 0, 0x60, 4]);
    a.jump(0x51, "right");
    a.jump(0x4d, "left");
    a.mark("end");
    a.emit(&[0x65]);
    a.mark("handler");
    a.emit(&[0x65]);
    a.mark("left");
    a.emit(&[0x60, 5, 0x29, 0]);
    a.jump(0x4d, "join");
    a.mark("right");
    a.emit(&[0x60, 6, 0x29, 0]);
    a.mark("join");
    a.emit(&[0x64]);
    let (bytes, regions) = a.finish("start", "end", "handler");
    let body = lifted(&bytes, &regions, 1);
    assert!(
        matches!(body.as_slice(), [Stmt::Comment(text)] if text.contains("unsupported instruction in exception CFG") && text.contains("createprivateproperty")),
        "{body:#?}"
    );
}

#[test]
fn normal_entry_to_shared_handler_keeps_original_error_and_nearest_outer_protection() {
    let mut a = Asm::default();
    a.int(99);
    a.emit(&[0x61, 0]);
    a.mark("outer_start");
    a.mark("start");
    a.emit(&[0x60, 6, 0x29, 0]);
    a.mark("end");
    a.jump(0x4d, "handler");
    // 成功和失败的 close 调用都重抛旧异常 v0，而不是当前 accumulator。
    a.mark("handler");
    a.emit(&[0x60, 0, 0xfe, 0x00]);
    a.mark("outer_end");
    a.jump(0x4d, "join");
    a.mark("outer_handler");
    a.emit(&[0x61, 1, 0x60, 1, 0x64]);
    a.mark("join");
    a.emit(&[0x65]);
    let outer = TryBlock {
        start_pc: a.labels["outer_start"] as u32,
        length: (a.labels["outer_end"] - a.labels["outer_start"]) as u32,
        catch_blocks: vec![CatchBlock {
            type_idx: 0,
            handler_pc: a.labels["outer_handler"] as u32,
            code_size: 0,
        }],
    };
    let (bytes, mut regions) = a.finish("start", "end", "handler");
    regions.push(outer);
    for throwing in [None, Some(0)] {
        compare(&bytes, &regions, 2, false, throwing, false);
    }
}

#[test]
fn hole_sentinel_does_not_swallow_user_thrown_undefined_or_coercible_string() {
    let mut a = Asm::default();
    a.emit(&[0x70, 0x61, 0]);
    a.mark("start");
    a.emit(&[0x60, 6, 0x29, 0]);
    a.mark("end");
    a.jump(0x4d, "join");
    a.mark("handler");
    a.emit(&[0x61, 0]);
    a.mark("join");
    a.emit(&[0x70, 0x61, 1, 0x60, 0, 0x10, 0, 1]);
    a.jump(0x51, "throw");
    a.int(1);
    a.emit(&[0x64]);
    a.mark("throw");
    a.emit(&[0x60, 0, 0xfe, 0x00]);
    let (bytes, regions) = a.finish("start", "end", "handler");
    let body = lifted(&bytes, &regions, 2);
    for thrown in [Value::Undefined, Value::String("[object Object]".into())] {
        let mut original = Execution::new(false, Some(0));
        original.thrown_value = Some(thrown.clone());
        let expected = original.bytecode(&bytes, &regions, 2);
        let mut recovered = Execution::new(false, Some(0));
        recovered.thrown_value = Some(thrown);
        let actual = recovered.stmts(&body);
        assert_eq!(
            (actual, recovered.calls),
            (expected, original.calls),
            "{body:#?}"
        );
    }
}

#[test]
fn mov_overwriting_register_alias_does_not_change_the_accumulator() {
    let mut a = Asm::default();
    a.int(1);
    a.emit(&[0x61, 0]);
    a.int(2);
    a.emit(&[0x61, 1]);
    a.emit(&[0x60, 0, 0x45, 0, 1, 0x64]);
    // 同一基本块内 v0 被覆盖，原 acc 仍为 1；第二例通过状态模式的 handler 路由。
    compare(&a.bytes, &[], 2, false, None, false);
    let mut b = Asm::default();
    b.int(1);
    b.emit(&[0x61, 0]);
    b.int(2);
    b.emit(&[0x61, 1]);
    b.mark("start");
    b.emit(&[0x60, 0, 0x45, 0, 1]);
    b.mark("end");
    b.jump(0x4d, "handler");
    b.mark("handler");
    b.emit(&[0x64]);
    let (bytes, regions) = b.finish("start", "end", "handler");
    compare(&bytes, &regions, 2, false, None, false);
    let mut c = Asm::default();
    c.int(1);
    c.emit(&[0x61, 0]);
    c.int(2);
    c.emit(&[0x61, 1]);
    c.int(3);
    c.emit(&[0x0a, 0, 0, 0x45, 0, 1, 0x64]);
    // 复合 lazy 表达式 1+3 也必须在 v0 被覆写为 2 前求值一次。
    compare(&c.bytes, &[], 2, false, None, false);
}

#[test]
fn nested_try_keeps_entry_effects_and_throwing_catch_under_outer_handler() {
    let mut a = Asm::default();
    a.mark("outer_start");
    a.emit(&[0x60, 6, 0x29, 0]);
    a.mark("start");
    a.emit(&[0x60, 5, 0x29, 0]);
    a.mark("end");
    a.jump(0x4d, "inner_join");
    // 相同回调在正常路径和 catch 再次抛出，用来验证外层处理器仍生效。
    a.mark("handler");
    a.emit(&[0x61, 0, 0x60, 5, 0x29, 0]);
    a.mark("inner_join");
    a.emit(&[0x60, 8, 0x29, 0]);
    a.mark("outer_end");
    a.jump(0x4d, "join");
    a.mark("outer_handler");
    a.emit(&[0x61, 0, 0x60, 7, 0x29, 0]);
    a.mark("join");
    a.emit(&[0x65]);
    let outer = TryBlock {
        start_pc: a.labels["outer_start"] as u32,
        length: (a.labels["outer_end"] - a.labels["outer_start"]) as u32,
        catch_blocks: vec![CatchBlock {
            type_idx: 0,
            handler_pc: a.labels["outer_handler"] as u32,
            code_size: 0,
        }],
    };
    let (bytes, mut regions) = a.finish("start", "end", "handler");
    regions.push(outer);
    for throwing in [None, Some(0), Some(1), Some(2), Some(3)] {
        compare(&bytes, &regions, 1, false, throwing, false);
    }
}
