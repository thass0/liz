#[derive(Debug)]
pub struct UserCode(String);

impl UserCode {
    pub fn new<S>(source: S) -> Self
    where
        S: AsRef<str>,
    {
        UserCode(String::from(source.as_ref()))
    }

    pub fn append<S>(&mut self, source: S)
    where
        S: AsRef<str>,
    {
        let indents = match self.balance() {
            Balanced::NoMissing(n) => n as usize,
            _ => 0,
        };

        let code = Self::extract_code(source.as_ref());
        for line in code.lines() {
            let mut chars = line.chars();
            while let Some(c) = chars.next() {
                match c {
                    ')' => self.0.push(')'),
                    _ => {
                        self.0.push('\n');
                        // This hacky way of adding tabs seems to be a good
                        // heuristic for making the code look decent while
                        // begin fast.
                        self.0.push_str(&"\t".repeat(indents));

                        self.0.push(c);
                        self.0.push_str(&chars.collect::<String>());
                        break;
                    },
                }
            }
        }
    }

    /// Are the parentheses in the source code balanced?
    pub fn balance(&self) -> Balanced {
        let mut n_opened: i32 = 0;
        for c in self.0.chars() {
            match c {
                '(' => n_opened += 1,
                ')' => n_opened -= 1,
                _ => {},
            }
        }
        match n_opened {
            0 => Balanced::Yes,
            i32::MIN..=-1 => Balanced::NoTrailing(n_opened.abs() as u32),
            1..=i32::MAX => Balanced::NoMissing(n_opened as u32),
        }
    }

    /// Remove Discord's formatting (i.e. backticks etc.)
    /// from `formatted` and return  only the source code
    /// part of the input.
    fn extract_code<'a>(code: &'a str) -> &'a str {
        // Strip optional prefixes.
        let s = code.trim().strip_prefix("```").map_or_else(
            || code.strip_prefix("`").unwrap_or(code),
            |s| s.strip_prefix("lisp\n").unwrap_or(s),
        );
        // Strip optional postfixes.
        let s = s
            .trim()
            .strip_suffix("```")
            .or_else(|| s.strip_suffix("`"))
            .unwrap_or(s);
        s.trim()
    }

    pub fn eval(&self) -> String {
        let mut env = LisEnv::new();
        for sexpr in parse(&self.0) {
            if let Ok(value) = sexpr {
                env.eval(&value);
            }
        }
        env.to_string()
    }
}

impl std::fmt::Display for UserCode {
    /// Add Discord's formatting to theh source
    /// code to display it nicely.
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "```lisp\n{}\n```", self.0)
    }
}

impl AsRef<str> for UserCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub enum Balanced {
    Yes,
    NoMissing(u32),
    NoTrailing(u32),
}

/// Evaluate a single S-expression.
pub fn eval_single<S>(src: S) -> String
where
    S: AsRef<str>,
{
    let sexpr_str = src.as_ref();
    let sexprs: Vec<Result<Value, ParseError>> = parse(sexpr_str).collect();
    match sexprs.len() {
        0 => "Missing S-expression".to_owned(),
        1 => match sexprs[0] {
            Ok(ref value) => {
                let mut env = LisEnv::new();
                env.eval(&value);
                env.to_string()
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

struct LisEnv {
    env:     Rc<RefCell<Env>>,
    output:  Rc<RefCell<String>>,
    results: Vec<(Result<Value, RuntimeError>, String)>,
}

impl LisEnv {
    fn new() -> Self {
        let mut env = default_env();

        // Register a custom print function that writes
        // to a per-env buffer instead of writing to the
        // server's stdout.
        let print = Symbol::from("print");
        env.undefine(&print);

        let output = Rc::new(RefCell::new(String::new()));
        let out_buf_ref = output.clone();
        let print_clo = Rc::new(RefCell::new(
            move |_env: Rc<RefCell<Env>>, args: Vec<Value>| {
                let expr = require_arg("print", &args, 0)?;
                let buf = &mut out_buf_ref.borrow_mut();
                let res = write!(buf, "{}\n", &expr);
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
            output,
            results: Vec::new(),
        }
    }

    fn eval(&mut self, sexpr: &Value) {
        let eval_res = interpreter::eval(self.env.clone(), sexpr);
        self.results.push((eval_res, self.output.borrow().clone()));
        self.output.borrow_mut().clear();
    }
}

impl std::fmt::Display for LisEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (value, printed) in self.results.iter() {
            if !printed.is_empty() {
                write!(f, "{}\n", printed)?;
            }
            match value {
                Ok(value) => {
                    let value = value.to_string();
                    if value.len() > 64 {
                        write!(
                            f,
                            "{}...{}",
                            &value[..32],
                            &value[(value.len() - 29)..]
                        )?;
                    } else {
                        write!(f, "{}", value)?;
                    }
                },
                Err(why) => write!(f, "{}", why)?,
            }
            write!(f, "\n")?;
        }
        Ok(())
    }
}

use std::cell::RefCell;
use std::fmt::Write;
use std::rc::Rc;

use rust_lisp::model::{Env, RuntimeError, Symbol, Value};
use rust_lisp::parser::{parse, ParseError};
use rust_lisp::utils::require_arg;
use rust_lisp::{default_env, interpreter};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_code_works() {
        // Any code works here, but I like the word 'blah'.
        assert_eq!(UserCode::extract_code("`blah`"), "blah");
        assert_eq!(UserCode::extract_code("`blah"), "blah");
        assert_eq!(UserCode::extract_code("blah`"), "blah");
        assert_eq!(UserCode::extract_code("```blah```"), "blah");
        assert_eq!(UserCode::extract_code("```blah"), "blah");
        assert_eq!(UserCode::extract_code("blah```"), "blah");
        assert_eq!(UserCode::extract_code("```lisp\nblah```"), "blah");
        assert_eq!(UserCode::extract_code("```lisp\nblah"), "blah");
        assert_eq!(UserCode::extract_code("lisp\nblah```"), "lisp\nblah");
    }

    #[test]
    fn append_code_works() {
        let mut code = UserCode::new(
            "(define fib (lambda (n)\n\t\t(if (< n 2)\n\t\t\tn(+ (fib (- n 1))",
        );
        code.append("(fib (- n 2))");
        assert!(code.0.ends_with("\n\t\t\t\t(fib (- n 2))"));
        code.append(")");
        assert!(code.0.ends_with("(- n 2)))"));
        code.append(")))");
        assert!(code.0.ends_with("(- n 2))))))"));
    }
}
