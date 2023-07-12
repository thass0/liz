use std::cell::RefCell;
use std::rc::Rc;

use rust_lisp::model::{Env, Symbol, Value};
use rust_lisp::parser::{parse, ParseError};
use rust_lisp::utils::require_arg;
use rust_lisp::{default_env, interpreter};

fn lisbot_eval_env() -> Env {
    let mut env = default_env();
    let print = Symbol::from("print");
    env.undefine(&print);
    env.define(
        print,
        Value::NativeFunc(|_env, args| {
            let expr = require_arg("print", &args, 0)?;
            // So ist print gerade die Identität der Werte.
            // Stattdessen sollte hier ein `write!`-Call in
            // einen pro-Nutzer/pro-Envocation buffer sein,
            // der dann in der Rückgabenachricht vor dem
            // Wert des Ausdrucks ist.
            Ok(expr.clone())
        }),
    );
    env
}

pub fn eval<S>(src: S) -> String
where
    S: AsRef<str>,
{
    let sexpr_str = src.as_ref();
    let sexprs: Vec<Result<Value, ParseError>> = parse(sexpr_str).collect();
    match sexprs.len() {
        0 => "Missing S-expression".to_owned(),
        1 => match sexprs[0] {
            Ok(ref value) => {
                let env = Rc::new(RefCell::new(lisbot_eval_env()));
                let res = interpreter::eval(env.clone(), &value);
                match res {
                    Ok(value) => value.to_string(),
                    Err(runtime_err) => runtime_err.to_string(),
                }
            },
            Err(ref parse_err) => {
                format!("Parse failed, {}", parse_err)
            },
        },
        len @ _ => {
            format!("Wrong number of S-expressions, {}", len)
        },
    }
}
