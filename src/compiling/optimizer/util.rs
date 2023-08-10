use crate::compiling::bytecode::{Bytecode, RegNum, Register};
use crate::compiling::opcodes::{Opcode, OpcodePos};

impl<T: RegNum> Opcode<Register<T>> {
    pub fn get_successors(&self, idx: usize) -> Vec<OpcodePos> {
        let mut successors = match *self {
            Opcode::Jump { to } => return vec![to],
            Opcode::JumpIfFalse { check, to } => vec![to],
            // Opcode::JumpIfEmpty { check, to } => vec![to],
            Opcode::UnwrapOrJump { check, to } => vec![to],
            Opcode::Return { src, module_ret } => return vec![],
            // Opcode::PushContextGroup { src } => todo!(),
            // Opcode::PopGroupStack { fn_reg } => todo!(),
            Opcode::YeetContext => return vec![],
            Opcode::EnterArrowStatement { skip } => vec![skip],
            Opcode::Throw { reg } => return vec![],
            _ => vec![],
        };
        successors.push((idx + 1).into());
        successors
    }

    pub fn get_read(self, code: &Bytecode<T>) -> Vec<Register<T>> {
        match self {
            Opcode::LoadConst { id, to } => vec![],
            Opcode::CopyDeep { from, to } => vec![from],
            Opcode::CopyShallow { from, to } => vec![from],
            Opcode::CopyRef { from, to } => vec![from],
            Opcode::Write { from, to } => vec![to, from],
            Opcode::WriteDeep { from, to } => vec![to, from],
            Opcode::AssignRef { from, to, left_mut } => vec![from],
            Opcode::AssignDeep { from, to, left_mut } => vec![to, from],
            Opcode::Plus { a, b, to } => vec![a, b],
            Opcode::Minus { a, b, to } => vec![a, b],
            Opcode::Mult { a, b, to } => vec![a, b],
            Opcode::Div { a, b, to } => vec![a, b],
            Opcode::Mod { a, b, to } => vec![a, b],
            Opcode::Pow { a, b, to } => vec![a, b],
            Opcode::Eq { a, b, to } => vec![a, b],
            Opcode::Neq { a, b, to } => vec![a, b],
            Opcode::Gt { a, b, to } => vec![a, b],
            Opcode::Gte { a, b, to } => vec![a, b],
            Opcode::Lt { a, b, to } => vec![a, b],
            Opcode::Lte { a, b, to } => vec![a, b],
            Opcode::BinOr { a, b, to } => vec![a, b],
            Opcode::BinAnd { a, b, to } => vec![a, b],
            Opcode::Range { a, b, to } => vec![a, b],
            Opcode::In { a, b, to } => vec![a, b],
            Opcode::ShiftLeft { a, b, to } => vec![a, b],
            Opcode::ShiftRight { a, b, to } => vec![a, b],
            Opcode::As { a, b, to } => vec![a, b],
            Opcode::PlusEq { a, b, left_mut } => vec![a, b],
            Opcode::MinusEq { a, b, left_mut } => vec![a, b],
            Opcode::MultEq { a, b, left_mut } => vec![a, b],
            Opcode::DivEq { a, b, left_mut } => vec![a, b],
            Opcode::PowEq { a, b, left_mut } => vec![a, b],
            Opcode::ModEq { a, b, left_mut } => vec![a, b],
            Opcode::BinAndEq { a, b, left_mut } => vec![a, b],
            Opcode::BinOrEq { a, b, left_mut } => vec![a, b],
            Opcode::ShiftLeftEq { a, b, left_mut } => vec![a, b],
            Opcode::ShiftRightEq { a, b, left_mut } => vec![a, b],
            Opcode::Not { v, to } => vec![v],
            Opcode::Negate { v, to } => vec![v],
            Opcode::PureEq { a, b, to } => vec![a, b],
            Opcode::PureNeq { a, b, to } => vec![a, b],
            Opcode::PureGte { a, b, to } => vec![a, b],
            Opcode::Jump { to } => vec![],
            Opcode::JumpIfFalse { check, to } => vec![check],
            Opcode::JumpIfTrue { check, to } => vec![check],
            Opcode::UnwrapOrJump { check, to } => vec![check],
            Opcode::IntoIterator { src, dest } => vec![src],
            Opcode::IterNext { src, dest } => vec![src],
            Opcode::AllocArray { dest, len } => vec![],
            Opcode::PushArrayElem { elem, dest } => vec![elem, dest],
            Opcode::AllocDict { dest, capacity } => vec![],
            Opcode::InsertDictElem { elem, dest, key } => vec![elem, dest, key],
            Opcode::InsertPrivDictElem { elem, dest, key } => vec![elem, dest, key],
            Opcode::MakeInstance { base, items, dest } => vec![base, items],
            Opcode::AllocObject { dest, capacity } => vec![],
            Opcode::AllocTrigger { dest, capacity } => vec![],
            Opcode::PushObjectElemKey {
                elem,
                obj_key,
                dest,
            } => vec![elem, dest],
            Opcode::PushObjectElemUnchecked {
                elem,
                obj_key,
                dest,
            } => vec![elem, dest],
            Opcode::EnterArrowStatement { skip } => vec![],
            Opcode::YeetContext => vec![],
            Opcode::LoadEmpty { to } => vec![],
            Opcode::LoadNone { to } => vec![],
            Opcode::LoadBuiltins { to } => vec![],
            Opcode::LoadEpsilon { to } => vec![],
            Opcode::LoadArbitraryID { class, dest } => vec![],
            Opcode::ApplyStringFlag { flag, reg } => vec![reg],
            Opcode::WrapMaybe { from, to } => vec![from],
            Opcode::Return { src, module_ret } => vec![src],
            Opcode::Dbg { reg, show_ptr } => vec![reg],
            Opcode::Throw { reg } => vec![reg],
            Opcode::Import { id, dest } => vec![],
            Opcode::ToString { from, dest } => vec![from],
            Opcode::Index { base, dest, index } => vec![base, index],
            Opcode::MemberImmut { from, dest, member } => vec![from, member],
            Opcode::MemberMut { from, dest, member } => vec![from, member],
            Opcode::Associated { from, dest, member } => vec![from, member],
            Opcode::TypeMember { from, dest, member } => vec![from, member],
            Opcode::TypeOf { src, dest } => vec![src],
            Opcode::Len { src, dest } => vec![src],
            Opcode::ArgAmount { src, dest } => vec![src],
            Opcode::MismatchThrowIfFalse {
                check_reg,
                value_reg,
            } => vec![check_reg, value_reg],
            Opcode::PushTryCatch { reg, to } => vec![reg],
            Opcode::PopTryCatch => vec![],
            Opcode::CreateMacro { func, dest } => code.functions[*func as usize]
                .captured_regs
                .iter()
                .map(|(r, _)| *r)
                .collect(),
            Opcode::PushMacroDefault { to, from, arg } => vec![to, from],
            Opcode::MarkMacroMethod { reg } => vec![reg],
            Opcode::Call { base, call } => {
                let mut v = vec![base];
                for &(r, _) in code.call_exprs[*call as usize].positional.iter() {
                    v.push(r)
                }
                for &(_, r, _) in code.call_exprs[*call as usize].named.iter() {
                    v.push(r)
                }
                v
            },
            Opcode::Impl { base, dict } => vec![base, dict],
            Opcode::RunBuiltin { args, dest } => (0..args).map(|g| Register(g.into())).collect(),
            Opcode::MakeTriggerFunc { src, dest } => vec![src],
            Opcode::CallTriggerFunc { func } => vec![func],
            Opcode::SetContextGroup { reg } => vec![reg],
            Opcode::AddOperatorOverload { from, op } => vec![from],
            Opcode::IncMismatchIdCount => vec![],
        }
    }

    pub fn get_write(self, code: &Bytecode<T>) -> Vec<Register<T>> {
        match self {
            Opcode::LoadConst { id, to } => vec![to],
            Opcode::CopyDeep { from, to } => vec![to],
            Opcode::CopyShallow { from, to } => vec![to],
            Opcode::CopyRef { from, to } => vec![to],
            Opcode::Write { from, to } => vec![to],
            Opcode::WriteDeep { from, to } => vec![to],
            Opcode::AssignRef { from, to, left_mut } => vec![to],
            Opcode::AssignDeep { from, to, left_mut } => vec![to],
            Opcode::Plus { a, b, to } => vec![to],
            Opcode::Minus { a, b, to } => vec![to],
            Opcode::Mult { a, b, to } => vec![to],
            Opcode::Div { a, b, to } => vec![to],
            Opcode::Mod { a, b, to } => vec![to],
            Opcode::Pow { a, b, to } => vec![to],
            Opcode::Eq { a, b, to } => vec![to],
            Opcode::Neq { a, b, to } => vec![to],
            Opcode::Gt { a, b, to } => vec![to],
            Opcode::Gte { a, b, to } => vec![to],
            Opcode::Lt { a, b, to } => vec![to],
            Opcode::Lte { a, b, to } => vec![to],
            Opcode::BinOr { a, b, to } => vec![to],
            Opcode::BinAnd { a, b, to } => vec![to],
            Opcode::Range { a, b, to } => vec![to],
            Opcode::In { a, b, to } => vec![to],
            Opcode::ShiftLeft { a, b, to } => vec![to],
            Opcode::ShiftRight { a, b, to } => vec![to],
            Opcode::As { a, b, to } => vec![to],
            Opcode::PlusEq { a, b, left_mut } => vec![a],
            Opcode::MinusEq { a, b, left_mut } => vec![a],
            Opcode::MultEq { a, b, left_mut } => vec![a],
            Opcode::DivEq { a, b, left_mut } => vec![a],
            Opcode::PowEq { a, b, left_mut } => vec![a],
            Opcode::ModEq { a, b, left_mut } => vec![a],
            Opcode::BinAndEq { a, b, left_mut } => vec![a],
            Opcode::BinOrEq { a, b, left_mut } => vec![a],
            Opcode::ShiftLeftEq { a, b, left_mut } => vec![a],
            Opcode::ShiftRightEq { a, b, left_mut } => vec![a],
            Opcode::Not { v, to } => vec![to],
            Opcode::Negate { v, to } => vec![to],
            Opcode::PureEq { a, b, to } => vec![to],
            Opcode::PureNeq { a, b, to } => vec![to],
            Opcode::PureGte { a, b, to } => vec![to],
            Opcode::Jump { to } => vec![],
            Opcode::JumpIfFalse { check, to } => vec![],
            Opcode::JumpIfTrue { check, to } => vec![],
            Opcode::UnwrapOrJump { check, to } => vec![check],
            Opcode::IntoIterator { src, dest } => vec![dest],
            Opcode::IterNext { src, dest } => vec![dest],
            Opcode::AllocArray { dest, len } => vec![dest],
            Opcode::PushArrayElem { elem, dest } => vec![dest],
            Opcode::AllocDict { dest, capacity } => vec![dest],
            Opcode::InsertDictElem { elem, dest, key } => vec![dest],
            Opcode::InsertPrivDictElem { elem, dest, key } => vec![dest],
            Opcode::MakeInstance { base, items, dest } => vec![dest],
            Opcode::AllocObject { dest, capacity } => vec![dest],
            Opcode::AllocTrigger { dest, capacity } => vec![dest],
            Opcode::PushObjectElemKey {
                elem,
                obj_key,
                dest,
            } => vec![dest],
            Opcode::PushObjectElemUnchecked {
                elem,
                obj_key,
                dest,
            } => vec![dest],
            Opcode::EnterArrowStatement { skip } => vec![],
            Opcode::YeetContext => vec![],
            Opcode::LoadEmpty { to } => vec![to],
            Opcode::LoadNone { to } => vec![to],
            Opcode::LoadBuiltins { to } => vec![to],
            Opcode::LoadEpsilon { to } => vec![to],
            Opcode::LoadArbitraryID { class, dest } => vec![dest],
            Opcode::ApplyStringFlag { flag, reg } => vec![reg],
            Opcode::WrapMaybe { from, to } => vec![to],
            Opcode::Return { src, module_ret } => vec![],
            Opcode::Dbg { reg, show_ptr } => vec![],
            Opcode::Throw { reg } => vec![],
            Opcode::Import { id, dest } => vec![dest],
            Opcode::ToString { from, dest } => vec![dest],
            Opcode::Index { base, dest, index } => vec![dest],
            Opcode::MemberImmut { from, dest, member } => vec![dest],
            Opcode::MemberMut { from, dest, member } => vec![dest],
            Opcode::Associated { from, dest, member } => vec![dest],
            Opcode::TypeMember { from, dest, member } => vec![dest],
            Opcode::TypeOf { src, dest } => vec![dest],
            Opcode::Len { src, dest } => vec![dest],
            Opcode::ArgAmount { src, dest } => vec![dest],
            Opcode::MismatchThrowIfFalse {
                check_reg,
                value_reg,
            } => vec![],
            Opcode::PushTryCatch { reg, to } => vec![],
            Opcode::PopTryCatch => vec![],
            Opcode::CreateMacro { func, dest } => vec![dest],
            Opcode::PushMacroDefault { to, from, arg } => vec![to],
            Opcode::MarkMacroMethod { reg } => vec![reg],
            Opcode::Call { base, call } => code.call_exprs[*call as usize]
                .dest
                .iter()
                .copied()
                .collect(),
            Opcode::Impl { base, dict } => vec![],
            Opcode::RunBuiltin { args, dest } => vec![dest],
            Opcode::MakeTriggerFunc { src, dest } => vec![dest],
            Opcode::CallTriggerFunc { func } => vec![],
            Opcode::SetContextGroup { reg } => vec![],
            Opcode::AddOperatorOverload { from, op } => vec![],
            Opcode::IncMismatchIdCount => vec![],
        }
    }
}
