use std::cell::RefCell;
use std::fmt::Write;
use std::rc::Rc;

use rust_lisp::model::{Env, RuntimeError, Symbol, Value};
use rust_lisp::parser::{parse, ParseError};
use rust_lisp::utils::require_arg;
use rust_lisp::{default_env, interpreter};

struct LisEnv {
    env:     Rc<RefCell<Env>>,
    out_buf: Rc<RefCell<String>>,
}

impl LisEnv {
    fn new() -> Self {
        let mut env = default_env();

        // Register a custom print function that writes
        // to a per-env buffer instead of writing to the
        // server's stdout.
        let print = Symbol::from("print");
        env.undefine(&print);

        let out_buf = Rc::new(RefCell::new(String::new()));
        let out_buf_ref = out_buf.clone();
        let print_clo = Rc::new(RefCell::new(
            move |_env: Rc<RefCell<Env>>, args: Vec<Value>| {
                let expr = require_arg("print", &args, 0)?;
                let out_buf: &mut String = &mut out_buf_ref.borrow_mut();
                let res = write!(out_buf, "{}\n", &expr);
                match res {
                    Ok(()) => Ok(expr.clone()),
                    Err(_) => Err(RuntimeError {
                        msg: "Failed to print output".to_owned(),
                    }),
                }
            },
        ));
        env.define(print, Value::NativeClosure(print_clo));

        LisEnv {
            env: Rc::new(RefCell::new(env)),
            out_buf,
        }
    }

    fn eval(&self, sexpr: &Value) -> Result<String, RuntimeError> {
        interpreter::eval(self.env.clone(), sexpr).map(|value| {
            // Preped the print output of the S-expression to its
            // return value.
            format!("{}{}", self.out_buf.borrow(), value.to_string())
        })
    }
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
                let env = LisEnv::new();
                match env.eval(&value) {
                    Ok(s) => s,
                    Err(e) => e.to_string(),
                }
            },
            Err(ref parse_err) => {
                format!("Invalid S-expression, {}", parse_err)
            },
        },
        len @ _ => {
            format!("Wrong number of S-expressions, {}", len)
        },
    }
}
