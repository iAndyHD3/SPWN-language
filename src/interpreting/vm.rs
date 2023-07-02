use std::cell::{Ref, RefCell, RefMut};
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::mem::{self, ManuallyDrop, MaybeUninit};
use std::rc::Rc;

use ahash::{AHashMap, RandomState};
use base64::Engine;
use colored::Colorize;
use derive_more::{Deref, DerefMut};
use itertools::Itertools;
use lasso::Spur;

use super::context::{CallInfo, Context, ContextSplitMode, ContextStack, FullContext, FuncStorage};
use super::value::{StoredValue, Value, ValueType};
use super::value_ops;
use crate::compiling::bytecode::{Bytecode, Constant, Function, OptRegister, UnoptRegister};
use crate::compiling::opcodes::{ConstID, Opcode, RuntimeStringFlag};
use crate::gd::gd_object::{make_spawn_trigger, TriggerObject, TriggerOrder};
use crate::gd::ids::{IDClass, Id};
use crate::interpreting::context::TryCatch;
use crate::interpreting::error::RuntimeError;
use crate::parsing::ast::{VisSource, VisTrait};
use crate::sources::{BytecodeMap, CodeArea, CodeSpan, Spanned, SpwnSource, ZEROSPAN};
use crate::util::Interner;

pub type RuntimeResult<T> = Result<T, RuntimeError>;

trait DeepClone<I> {
    fn deep_clone(&self, input: I) -> StoredValue;
    fn deep_clone_ref(&self, input: I) -> ValueRef {
        let v: StoredValue = self.deep_clone(input);
        ValueRef::new(v)
    }
}

#[derive(Debug, Clone, Deref, DerefMut)]
pub struct ValueRef(Rc<RefCell<StoredValue>>);

impl PartialEq for ValueRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for ValueRef {}

impl ValueRef {
    pub fn new(v: StoredValue) -> Self {
        Self(Rc::new(RefCell::new(v)))
    }
}

#[derive(Debug)]
pub struct Program {
    pub src: Rc<SpwnSource>,
    pub bytecode: Bytecode,
}

impl Program {
    pub fn get_constant(&self, id: ConstID) -> &Constant {
        &self.bytecode.constants[*id as usize]
    }

    pub fn get_function(&self, id: usize) -> &Function {
        &self.bytecode.functions[id]
    }
}

#[derive(Debug, Clone)]
pub struct FuncCoord {
    pub program: Rc<Program>,
    pub func: usize,
}

impl PartialEq for FuncCoord {
    fn eq(&self, other: &Self) -> bool {
        self.func == other.func && Rc::ptr_eq(&self.program, &other.program)
    }
}
impl Eq for FuncCoord {}

pub struct Vm {
    contexts: ContextStack,

    is_doc_gen: bool,

    pub triggers: Vec<TriggerObject>,
    pub trigger_order_count: TriggerOrder,

    pub id_counters: [u16; 4],
}

impl Vm {
    pub fn new(is_doc_gen: bool) -> Self {
        Self {
            contexts: ContextStack(vec![]),
            is_doc_gen,
            triggers: vec![],
            trigger_order_count: TriggerOrder::new(),
            id_counters: Default::default(),
        }
    }

    pub fn make_area(&self, span: CodeSpan, program: &Rc<Program>) -> CodeArea {
        CodeArea {
            span,
            src: Rc::clone(&program.src),
        }
    }

    pub fn set_reg(&mut self, reg: OptRegister, v: StoredValue) {
        let mut binding = self.contexts.current_mut();
        let mut g = binding.stack.last_mut().unwrap().registers[reg.0 as usize].borrow_mut();
        *g = v;
    }

    pub fn borrow_reg<F, R>(&self, reg: OptRegister, f: F) -> RuntimeResult<R>
    where
        F: FnOnce(Ref<'_, StoredValue>) -> RuntimeResult<R>,
    {
        f(self.contexts.current().stack.last().unwrap().registers[*reg as usize].borrow())
    }

    pub fn borrow_reg_mut<F, R>(&self, reg: OptRegister, f: F) -> RuntimeResult<R>
    where
        F: FnOnce(RefMut<'_, StoredValue>) -> RuntimeResult<R>,
    {
        f(self.contexts.current().stack.last().unwrap().registers[*reg as usize].borrow_mut())
    }

    pub fn get_reg_ref(&self, reg: OptRegister) -> &ValueRef {
        &self.contexts.current().stack.last().unwrap().registers[*reg as usize]
    }

    pub fn change_reg_ref(&mut self, reg: OptRegister, k: ValueRef) {
        *self
            .contexts
            .current_mut()
            .stack
            .last_mut()
            .unwrap()
            .registers[*reg as usize] = k
    }

    pub fn next_id(&mut self, c: IDClass) -> u16 {
        self.id_counters[c as usize] += 1;
        self.id_counters[c as usize]
    }
}

impl DeepClone<&ValueRef> for Vm {
    fn deep_clone(&self, input: &ValueRef) -> StoredValue {
        let v = &input.borrow();
        let area = v.area.clone();

        let value = match &v.value {
            Value::Array(arr) => Value::Array(arr.iter().map(|v| self.deep_clone_ref(v)).collect()),
            Value::Dict { .. } => todo!(),
            Value::Maybe { .. } => todo!(),
            Value::Instance { .. } => todo!(),
            Value::Module { .. } => todo!(),
            // todo: iterator, object
            v => v.clone(),
        };

        value.into_stored(area)
    }
}
impl DeepClone<OptRegister> for Vm {
    fn deep_clone(&self, input: OptRegister) -> StoredValue {
        let v = &self.contexts.current().stack.last().unwrap().registers[*input as usize];
        self.deep_clone(&**v)
    }
}

impl Vm {
    pub fn get_call_stack(&self) -> Vec<CallInfo> {
        self.contexts
            .0
            .iter()
            .map(|f| &f.call_info)
            .cloned()
            .collect()
    }

    /// <img src="https://cdna.artstation.com/p/assets/images/images/056/833/046/original/lara-hughes-blahaj-spin-compressed.gif?1670214805" width=60 height=60>
    /// <img src="https://cdna.artstation.com/p/assets/images/images/056/833/046/original/lara-hughes-blahaj-spin-compressed.gif?1670214805" width=60 height=60>
    /// <img src="https://cdna.artstation.com/p/assets/images/images/056/833/046/original/lara-hughes-blahaj-spin-compressed.gif?1670214805" width=60 height=60>
    pub fn run_function(
        &mut self,
        mut context: Context,
        call_info: CallInfo,
        split_mode: ContextSplitMode,
    ) -> RuntimeResult<()> {
        let CallInfo {
            func: FuncCoord { program, func },
            return_dest,
            ..
        } = call_info.clone();

        let original_ip = context.ip;
        context.ip = 0;

        {
            let mut regs = unsafe {
                std::mem::transmute::<_, [ManuallyDrop<ValueRef>; 256]>(
                    [[0u8; std::mem::size_of::<ManuallyDrop<ValueRef>>()]; 256],
                )
            };

            for i in 0..program.get_function(func).regs_used {
                // println!("zz");
                let v = ValueRef::new(StoredValue {
                    value: Value::Empty,
                    area: CodeArea {
                        src: Rc::clone(&program.src),
                        span: CodeSpan::internal(),
                    },
                });
                regs[i as usize] = ManuallyDrop::new(v);
            }

            context.stack.push(FuncStorage { registers: regs });
        }

        self.contexts.push(FullContext::new(context, call_info));
        let opcodes = &program.get_function(func).opcodes;
        let mut has_implicitly_returned = false;

        while self.contexts.valid() {
            let ip = self.contexts.ip();

            if ip >= opcodes.len() {
                let ip = self.contexts.ip();

                if ip >= opcodes.len() {
                    if !self.contexts.last().have_returned {
                        if has_implicitly_returned && split_mode == ContextSplitMode::Disallow {
                            // println!(
                            //     "africa asia europe antarctica north america south america {:?}",
                            //     program.bytecode.functions[*func].span
                            // );
                            return Err(RuntimeError::ContextSplitDisallowed {
                                area: self.make_area(program.get_function(func).span, &program),
                                call_stack: self.get_call_stack(),
                            });
                        }

                        let return_dest = self.contexts.last().call_info.return_dest;
                        {
                            let mut current = self.contexts.current_mut();

                            if let Some(mut regs) = current.stack.pop() {
                                for i in 0..program.get_function(func).regs_used {
                                    unsafe { ManuallyDrop::drop(&mut regs.registers[i as usize]) }
                                }
                                // std::mem::forget(regs);
                            }
                        }

                        let mut top = self.contexts.last_mut().yeet_current().unwrap();
                        top.ip = original_ip + 1;

                        if return_dest.is_some() {
                            let idx = self.contexts.0.len() - 2;
                            self.contexts.0[idx].contexts.push(top);
                        }
                        has_implicitly_returned = true;

                        continue;
                    } else {
                        self.contexts.yeet_current();
                    }
                    continue;
                }
            }

            let Spanned {
                value: opcode,
                span: opcode_span,
            } = opcodes[ip];

            macro_rules! load_val {
                ($val:expr, $to:expr) => {
                    self.set_reg($to, $val.into_stored(self.make_area(opcode_span, &program)))
                };
            }

            #[derive(Debug, Clone, Copy)]
            pub enum LoopFlow {
                ContinueLoop,
                Normal,
            }

            // MaybeUninit
            let mut run_opcode = |opcode| -> RuntimeResult<LoopFlow> {
                match opcode {
                    Opcode::LoadConst { id, to } => {
                        let value = Value::from_const(self, program.get_constant(id));
                        self.set_reg(to, value.into_stored(self.make_area(opcode_span, &program)));
                    },
                    Opcode::Copy { from, to } => self.set_reg(to, self.deep_clone(from)),
                    Opcode::CopyMem { from, to } => self.set_reg(to, self.deep_clone(from)),

                    Opcode::Plus { a, b, to } => {
                        self.bin_op(value_ops::plus, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Minus { a, b, to } => {
                        self.bin_op(value_ops::minus, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Mult { a, b, to } => {
                        self.bin_op(value_ops::mult, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Div { a, b, to } => {
                        self.bin_op(value_ops::div, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Mod { a, b, to } => {
                        self.bin_op(value_ops::modulo, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Pow { a, b, to } => {
                        self.bin_op(value_ops::pow, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Eq { a, b, to } => {
                        self.bin_op(value_ops::eq_op, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Neq { a, b, to } => {
                        self.bin_op(value_ops::neq_op, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Gt { a, b, to } => {
                        self.bin_op(value_ops::gt, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Gte { a, b, to } => {
                        self.bin_op(value_ops::gte, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Lt { a, b, to } => {
                        self.bin_op(value_ops::lt, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Lte { a, b, to } => {
                        self.bin_op(value_ops::lte, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::BinOr { a, b, to } => {
                        self.bin_op(value_ops::bin_or, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::BinAnd { a, b, to } => {
                        self.bin_op(value_ops::bin_and, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::Range { a, b, to } => {
                        self.bin_op(value_ops::range, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::In { a, b, to } => {
                        self.bin_op(value_ops::in_op, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::ShiftLeft { a, b, to } => {
                        self.bin_op(value_ops::shift_left, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::ShiftRight { a, b, to } => {
                        self.bin_op(value_ops::shift_right, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::As { a, b, to } => {
                        self.bin_op(value_ops::as_op, &program, a, b, to, opcode_span)?;
                    },
                    Opcode::PlusEq { a, b } => {
                        self.assign_op(value_ops::plus, &program, a, b, opcode_span)?;
                    },
                    Opcode::MinusEq { a, b } => {
                        self.assign_op(value_ops::minus, &program, a, b, opcode_span)?;
                    },
                    Opcode::MultEq { a, b } => {
                        self.assign_op(value_ops::plus, &program, a, b, opcode_span)?;
                    },
                    Opcode::DivEq { a, b } => {
                        self.assign_op(value_ops::mult, &program, a, b, opcode_span)?;
                    },
                    Opcode::PowEq { a, b } => {
                        self.assign_op(value_ops::pow, &program, a, b, opcode_span)?;
                    },
                    Opcode::ModEq { a, b } => {
                        self.assign_op(value_ops::modulo, &program, a, b, opcode_span)?;
                    },
                    Opcode::BinAndEq { a, b } => {
                        self.assign_op(value_ops::bin_and, &program, a, b, opcode_span)?;
                    },
                    Opcode::BinOrEq { a, b } => {
                        self.assign_op(value_ops::bin_or, &program, a, b, opcode_span)?;
                    },
                    Opcode::ShiftLeftEq { a, b } => {
                        self.assign_op(value_ops::shift_left, &program, a, b, opcode_span)?;
                    },
                    Opcode::ShiftRightEq { a, b } => {
                        self.assign_op(value_ops::shift_right, &program, a, b, opcode_span)?;
                    },
                    Opcode::Not { v, to } => {
                        self.unary_op(value_ops::unary_not, &program, v, to, opcode_span)?;
                    },
                    Opcode::Negate { v, to } => {
                        self.unary_op(value_ops::unary_negate, &program, v, to, opcode_span)?;
                    },
                    Opcode::Jump { to } => {
                        self.contexts.jump_current(*to as usize);
                        return Ok(LoopFlow::ContinueLoop);
                    },
                    Opcode::JumpIfFalse { check, to } => {
                        let b = self.borrow_reg(check, |check| {
                            value_ops::to_bool(&check, opcode_span, self, &program)
                        })?;

                        if !b {
                            self.contexts.jump_current(*to as usize);
                            return Ok(LoopFlow::ContinueLoop);
                        }
                    },
                    Opcode::JumpIfTrue { check, to } => {
                        let b = self.borrow_reg(check, |check| {
                            value_ops::to_bool(&check, opcode_span, self, &program)
                        })?;

                        if b {
                            self.contexts.jump_current(*to as usize);
                            return Ok(LoopFlow::ContinueLoop);
                        }
                    },
                    Opcode::UnwrapOrJump { check, to } => {
                        match &self.get_reg_ref(check).clone().borrow().value {
                            Value::Maybe(v) => match v {
                                Some(k) => {
                                    let val = self.deep_clone(k);

                                    self.set_reg(check, val);
                                },
                                None => {
                                    self.contexts.jump_current(*to as usize);
                                    return Ok(LoopFlow::ContinueLoop);
                                },
                            },
                            _ => unreachable!(),
                        };
                    },
                    Opcode::AllocArray { dest, len } => self.set_reg(
                        dest,
                        StoredValue {
                            value: Value::Array(Vec::with_capacity(len as usize)),
                            area: self.make_area(opcode_span, &program),
                        },
                    ),
                    Opcode::PushArrayElem { elem, dest } => {
                        let push = self.deep_clone_ref(elem);
                        match &mut self.get_reg_ref(dest).borrow_mut().value {
                            Value::Array(v) => v.push(push),
                            _ => unreachable!(),
                        }
                    },
                    Opcode::AllocDict { dest, capacity } => self.set_reg(
                        dest,
                        StoredValue {
                            value: Value::Dict(AHashMap::with_capacity(capacity as usize)),
                            area: self.make_area(opcode_span, &program),
                        },
                    ),
                    Opcode::InsertDictElem { elem, dest, key } => {
                        let push = self.deep_clone_ref(elem);

                        let key = self.borrow_reg(key, |key| match &key.value {
                            Value::String(s) => Ok(Rc::clone(s)),
                            _ => unreachable!(),
                        })?;

                        match &mut self.get_reg_ref(dest).borrow_mut().value {
                            Value::Dict(v) => v.insert(key, VisSource::Public(push)),
                            _ => unreachable!(),
                        };
                    },
                    Opcode::InsertPrivDictElem { elem, dest, key } => {
                        let push = self.deep_clone_ref(elem);

                        let key = self.borrow_reg(key, |key| match &key.value {
                            Value::String(s) => Ok(Rc::clone(s)),
                            _ => unreachable!(),
                        })?;

                        match &mut self.get_reg_ref(dest).borrow_mut().value {
                            Value::Dict(v) => {
                                v.insert(key, VisSource::Private(push, Rc::clone(&program.src)))
                            },
                            _ => unreachable!(),
                        };
                    },
                    Opcode::WrapIterator { src, dest } => todo!(),
                    Opcode::IterNext { src, dest } => todo!(),
                    Opcode::Return { src, module_ret } => {
                        // let mut ret_val = self.deep_clone(src);

                        // if module_ret {
                        //     match ret_val.value {
                        //         Value::Dict(d) => {
                        //             ret_val.value = Value::Module {
                        //                 exports: d.into_iter().map(|(s, v)| (s, *v)).collect(),
                        //                 types: self.programs[func.code].2.to_vec(),
                        //             }
                        //         },
                        //         _ => unreachable!(),
                        //     }
                        // }

                        // if self.contexts.last().have_returned
                        //     && split_mode == ContextSplitMode::Disallow
                        // {
                        //     let span = self.get_span(func, ip);

                        //     return Err(RuntimeError::ContextSplitDisallowed {
                        //         area: self.make_area(span, func.code),
                        //         call_stack: self.get_call_stack(),
                        //     });
                        // }

                        // self.contexts.last_mut().have_returned = true;

                        // let return_dest = self.contexts.last().call_info.return_dest;
                        // {
                        //     let mut current = self.contexts.current_mut();
                        //     current.registers.pop();

                        //     if let Some(r) = return_dest {
                        //         self.memory[current.registers.last_mut().unwrap()[r as usize]] =
                        //             ret_val.into_collect();
                        //     }
                        // }

                        // let mut top = self.contexts.last_mut().yeet_current().unwrap();
                        // top.ip = original_ip + 1;

                        // if return_dest.is_some() {
                        //     // dbg!(&self.contexts);
                        //     let idx = self.contexts.0.len() - 2;
                        //     self.contexts.0[idx].contexts.push(top);
                        // }

                        // continue;
                    },
                    Opcode::Import { id, dest } => todo!(),
                    Opcode::EnterArrowStatement { skip } => todo!(),
                    Opcode::YeetContext => todo!(),
                    Opcode::Dbg { reg } => {
                        let value_ref = self.get_reg_ref(reg).borrow();

                        println!(
                            "{} {} {}",
                            value_ref.value.runtime_display(self),
                            "::".dimmed(),
                            self.contexts.group().fmt("g").green(),
                        )
                    },
                    Opcode::Throw { reg } => {
                        let value = self.deep_clone(reg);

                        return Err(RuntimeError::ThrownError {
                            area: self.make_area(opcode_span, &program),
                            value,
                            call_stack: self.get_call_stack(),
                        });
                    },
                    Opcode::Index { base, dest, index } => {
                        let base_ref = self.get_reg_ref(base).borrow();
                        let index_ref = self.get_reg_ref(index).borrow();

                        let index_wrap = |idx: i64, len: usize, typ: ValueType| {
                            let index_calc = if idx >= 0 { idx } else { len as i64 + idx };

                            if index_calc < 0 || index_calc >= len as i64 {
                                return Err(RuntimeError::IndexOutOfBounds {
                                    len,
                                    index: idx,
                                    area: self.make_area(opcode_span, &program),
                                    typ,
                                    call_stack: self.get_call_stack(),
                                });
                            }

                            Ok(index_calc as usize)
                        };

                        match (&base_ref.value, &index_ref.value) {
                            (Value::Array(v), Value::Int(index)) => {
                                let k = &v[index_wrap(*index, v.len(), ValueType::Array)?];
                                let k = self.deep_clone(k);

                                std::mem::drop(base_ref);
                                std::mem::drop(index_ref);

                                self.set_reg(dest, k);
                            },
                            (Value::String(s), Value::Int(index)) => {
                                let idx = index_wrap(*index, s.len(), ValueType::String)?;
                                let c = s[idx];

                                std::mem::drop(base_ref);
                                std::mem::drop(index_ref);

                                self.set_reg(
                                    dest,
                                    Value::String(Rc::new([c]))
                                        .into_stored(self.make_area(opcode_span, &program)),
                                );
                            },
                            (Value::Dict(v), Value::String(s)) => match v.get(s) {
                                Some(v) => {
                                    let v = self.deep_clone(v.value());
                                    std::mem::drop(base_ref);
                                    std::mem::drop(index_ref);

                                    self.set_reg(dest, v);
                                },
                                None => {
                                    return Err(RuntimeError::NonexistentMember {
                                        area: self.make_area(opcode_span, &program),
                                        member: s.iter().collect(),
                                        base_type: base_ref.value.get_type(),
                                        call_stack: self.get_call_stack(),
                                    })
                                },
                            },
                            _ => {
                                return Err(RuntimeError::InvalidIndex {
                                    base: (base_ref.value.get_type(), base_ref.area.clone()),
                                    index: (index_ref.value.get_type(), index_ref.area.clone()),
                                    area: self.make_area(opcode_span, &program),
                                    call_stack: self.get_call_stack(),
                                });
                            },
                        };
                    },
                    Opcode::Member { from, dest, member } => todo!(),
                    Opcode::TypeOf { src, dest } => todo!(),
                    Opcode::Len { src, dest } => todo!(),
                    Opcode::IndexMem { base, dest, index } => todo!(),
                    Opcode::MemberMem { from, dest, member } => todo!(),
                    Opcode::AssociatedMem { from, dest, member } => todo!(),
                    Opcode::MismatchThrowIfFalse { reg } => {
                        //
                    },
                    Opcode::WrapMaybe { from, to } => todo!(),
                    Opcode::TypeMember { from, dest, member } => todo!(),

                    Opcode::LoadEmpty { to } => load_val!(Value::Empty, to),
                    Opcode::LoadNone { to } => load_val!(Value::Maybe(None), to),
                    Opcode::LoadBuiltins { to } => load_val!(Value::Builtins, to),
                    Opcode::LoadEpsilon { to } => load_val!(Value::Epsilon, to),

                    Opcode::LoadArbitraryID { class, dest } => {
                        let id = Id::Arbitrary(self.next_id(class));
                        let v = match class {
                            IDClass::Group => Value::Group(id),
                            IDClass::Channel => Value::Channel(id),
                            IDClass::Block => Value::Block(id),
                            IDClass::Item => Value::Item(id),
                        };

                        self.set_reg(
                            dest,
                            StoredValue {
                                value: v,
                                area: self.make_area(opcode_span, &program),
                            },
                        )
                    },

                    Opcode::ApplyStringFlag { flag, reg } => {
                        let area = self.make_area(opcode_span, &program);

                        let val = match &self.get_reg_ref(reg).borrow().value {
                            Value::String(s) => {
                                let mut v = vec![];

                                match flag {
                                    RuntimeStringFlag::ByteString => {
                                        for c in s.iter() {
                                            let mut buf = [0u8; 4];

                                            for b in c.encode_utf8(&mut buf).bytes() {
                                                v.push(ValueRef::new(
                                                    Value::Int(b as i64).into_stored(area.clone()),
                                                ))
                                            }
                                        }

                                        Value::Array(v)
                                    },
                                    RuntimeStringFlag::Unindent => {
                                        let s = unindent::unindent(&s.iter().collect::<String>());

                                        Value::String(s.chars().collect_vec().into())
                                    },
                                    RuntimeStringFlag::Base64 => {
                                        let s = base64::engine::general_purpose::URL_SAFE
                                            .encode(&s.iter().collect::<String>());

                                        Value::String(s.chars().collect_vec().into())
                                    },
                                }
                            },
                            _ => unreachable!(),
                        };

                        self.set_reg(reg, val.into_stored(area))
                    },
                    Opcode::Associated { from, dest, member } => todo!(),
                    Opcode::ToString { from, dest } => {
                        let s = self.get_reg_ref(from).borrow().value.runtime_display(self);
                        self.set_reg(
                            dest,
                            Value::String(s.chars().collect())
                                .into_stored(self.make_area(opcode_span, &program)),
                        )
                    },
                    Opcode::MakeInstance { base, items, dest } => todo!(),
                    Opcode::PushTryCatch { reg, to } => {
                        self.contexts
                            .current_mut()
                            .try_catches
                            .push(TryCatch { jump_pos: to, reg });
                    },
                    Opcode::PopTryCatch => {
                        self.contexts.current_mut().try_catches.pop();
                    },
                }
                Ok(LoopFlow::Normal)
            };

            match run_opcode(opcode) {
                Ok(flow) => match flow {
                    LoopFlow::ContinueLoop => {
                        continue;
                    },
                    LoopFlow::Normal => {},
                },
                Err(err) => {
                    let t = self.contexts.current_mut().try_catches.pop();
                    if let Some(try_catch) = t {
                        assert_eq!(
                            std::mem::size_of::<std::mem::Discriminant<RuntimeError>>(),
                            std::mem::size_of::<u64>()
                        );

                        let val = match err {
                            RuntimeError::ThrownError { value, .. } => value,
                            _ => Value::Error(unsafe {
                                std::mem::transmute::<_, u64>(std::mem::discriminant(&err)) as usize
                            })
                            .into_stored(self.make_area(ZEROSPAN, &program)),
                        };

                        self.set_reg(try_catch.reg, val);

                        self.contexts.jump_current(*try_catch.jump_pos as usize);
                        continue;
                    } else {
                        Err(err)?;
                    }
                },
            }

            {
                let mut current = self.contexts.current_mut();
                current.ip += 1;
            };
            self.try_merge_contexts();
        }

        Ok(())
    }

    pub fn hash_value(&self, val: &ValueRef, state: &mut DefaultHasher) {
        // hash numbers
        if let Value::Int(a) = val.borrow().value {
            mem::discriminant(&Value::Float(0.0)).hash(state);
            (a as f64).to_bits().hash(state);
            return;
            // convert all ints to floats so float-int equality works
        };

        // hash enum discriminator
        mem::discriminant(&val.borrow().value).hash(state);

        // todo: rest
        match &val.borrow().value {
            Value::Int(_) => unreachable!(),
            Value::Float(a) => a.to_bits().hash(state),
            Value::Bool(a) => a.hash(state),
            Value::String(a) => a.hash(state),
            Value::Array(a) => {
                for v in a {
                    self.hash_value(v, state)
                }
            },
            Value::Dict(a) => {
                let a: BTreeMap<_, _> = a.iter().collect();
                for (k, v) in a {
                    self.hash_value(v.value(), state);
                    v.source().hash(state);
                    k.hash(state);
                }
            },
            Value::Group(a) => a.hash(state),
            Value::Channel(a) => a.hash(state),
            Value::Block(a) => a.hash(state),
            Value::Item(a) => a.hash(state),
            Value::Builtins => (),
            Value::Range(a, b, c) => (a, b, c).hash(state),
            Value::Maybe(a) => {
                if let Some(v) = a {
                    self.hash_value(v, state)
                }
            },
            Value::Empty => (),
            Value::Type(a) => a.hash(state),
            Value::Module { .. } => (),
            Value::TriggerFunction {
                group: g,
                prev_context: p,
            } => (g, p).hash(state),
            Value::Error(a) => a.hash(state),
            Value::Epsilon => (),
            Value::Instance { typ, items } => {
                let items: BTreeMap<_, _> = items.iter().collect();
                typ.hash(state);
                for (k, v) in items {
                    self.hash_value(v.value(), state);
                    v.source().hash(state);
                    k.hash(state);
                }
            },
            Value::Chroma { r, g, b, a } => (r, g, b, a).hash(state),
        }
    }

    fn try_merge_contexts(&mut self) {
        let mut top = vec![];
        {
            let full_ctx = self.contexts.last_mut();

            if full_ctx.contexts.len() <= 1 {
                return;
            }

            // group by pos

            let top_ip = full_ctx.current().ip;
            loop {
                if full_ctx.contexts.is_empty() {
                    break;
                }
                if full_ctx.current().ip == top_ip {
                    top.push(full_ctx.contexts.pop().unwrap());
                } else {
                    break;
                }
            }
        }

        if top.len() > 1 {
            // hash the contexts
            let mut hashes = AHashMap::new();
            for ctx in top {
                let mut state = DefaultHasher::default();
                for val in &ctx.stack.last().unwrap().registers {
                    if unsafe {
                        std::mem::transmute::<_, u64>(std::ptr::read(val as *const _)) == 0
                    } {
                        break;
                    }
                    self.hash_value(val, &mut state);
                }
                let hash = state.finish();
                hashes.entry(hash).or_insert_with(Vec::new).push(ctx);
            }

            // merge the contexts
            for (_, ctxs) in hashes {
                if ctxs.len() > 1 {
                    let mut iter = ctxs.into_iter();
                    let ctx = iter.next().unwrap();
                    let target = ctx.group;
                    for ctx2 in iter {
                        // add a spawn trigger in this context to the merged context
                        let trigger = make_spawn_trigger(ctx2.group, target, self);
                        self.triggers.push(trigger);
                    }
                    self.contexts.last_mut().contexts.push(ctx);
                } else {
                    self.contexts.last_mut().contexts.extend(ctxs);
                }
            }
        } else {
            self.contexts.last_mut().contexts.extend(top);
        }
    }

    #[inline]
    pub fn bin_op(
        &mut self,
        op: fn(&StoredValue, &StoredValue, CodeSpan, &Vm, &Rc<Program>) -> RuntimeResult<Value>,
        program: &Rc<Program>,
        left: OptRegister,
        right: OptRegister,
        dest: OptRegister,
        span: CodeSpan,
    ) -> Result<(), RuntimeError> {
        // TODO: overloads

        #[rustfmt::skip]
        let value = self.borrow_reg(left, |left| {
            self.borrow_reg(right, |right| {
                op(
                    &left,
                    &right,
                    span,
                    self,
                    program,
                )
            })
        })?;

        self.set_reg(
            dest,
            StoredValue {
                value,
                area: self.make_area(span, program),
            },
        );
        Ok(())
    }

    #[inline]
    fn assign_op(
        &mut self,
        op: fn(&StoredValue, &StoredValue, CodeSpan, &Vm, &Rc<Program>) -> RuntimeResult<Value>,
        program: &Rc<Program>,
        left: OptRegister,
        right: OptRegister,
        span: CodeSpan,
    ) -> Result<(), RuntimeError> {
        #[rustfmt::skip]
        let value = self.borrow_reg(left, |left| {
            self.borrow_reg(right, |right| {
                op(
                    &left,
                    &right,
                    span,
                    self,
                    program,
                )
            })
        })?;

        self.borrow_reg_mut(left, |mut v| {
            v.value = value;
            Ok(())
        })?;
        Ok(())
    }

    #[inline]
    fn unary_op(
        &mut self,
        op: fn(&StoredValue, CodeSpan, &Vm, &Rc<Program>) -> RuntimeResult<Value>,
        program: &Rc<Program>,
        value: OptRegister,
        dest: OptRegister,
        span: CodeSpan,
    ) -> Result<(), RuntimeError> {
        #[rustfmt::skip]
        let value = self.borrow_reg(value, |value| {
            op(
                &value,
                span,
                self,
                program,
            )
        })?;

        self.set_reg(
            dest,
            StoredValue {
                value,
                area: self.make_area(span, program),
            },
        );
        Ok(())
    }

    pub fn convert_type(
        &self,
        v: &StoredValue,
        b: ValueType,
        span: CodeSpan, // ✍️
        program: &Rc<Program>,
    ) -> RuntimeResult<Value> {
        if v.value.get_type() == b {
            return Ok(v.value.clone());
        }

        Ok(match (&v.value, b) {
            _ => todo!("error"),
        })
    }
}
