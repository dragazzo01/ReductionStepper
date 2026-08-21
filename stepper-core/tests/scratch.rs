mod common;

use common::*;
use stepper_core::{parse_program, pretty::pretty_print, stepping, typecheck};

fn show_it(src: &str) {
    stepping::reset_rec_env();
    match parse_program(src) {
        Ok(p) => {
            println!("  {src}\n=> {}", pretty_print(&p));
            match typecheck(&p) {
                None => println!("=> runs to: {}", run_to_value(p)),
                Some(e) => println!("=> TYPE ERROR: {e}"),
            }
        }
        Err(e) => println!("  {src}\n=> PARSE ERROR: {e}"),
    }
    println!();
}

#[test]
fn scratch() {
    // fresh argument names must dodge anything the clauses mention
    show_it("val argA = 100\nfun k 0 (y : int) = argA | k (x : int) (y : int) = x\nval q = k 0 1");
    // partial application of the general derived form
    show_it("fun k 0 (y : int) = y | k (x : int) (y : int) = x * y\nval p : int -> int = k 3\nval q = p 4");
    // a bool result, and a fun whose body is a case
    show_it("fun isZero (n : int) = case n of 0 => true | _ => false\nval b = isZero 3");
    // nested funs
    show_it("fun outer (n : int) : int = let fun inner (m : int) : int = if m = 0 then 0 else m + inner (m - 1) in inner n end\nval z = outer 3");
    // a clause body that ends in a case, followed by another clause
    show_it("fun f 0 = case 1 of _ => 1 | f (n : int) : int = n");
    // mutual recursion via `and` is not supported
    show_it("fun f (x : int) : int = g x and g (y : int) : int = y");
    // tuple argument, recursive
    show_it("fun sum (0, acc : int) : int = acc | sum (n : int, acc : int) = sum (n - 1, acc + n)\nval s = sum (4, 0)");
    // zero-argument fun is not a thing
    show_it("fun f = 3");
    // fn now accepts a component-annotated tuple parameter too
    show_it("val f = fn (x : int, y : int) => x + y\nval v = f (1, 2)");
}
