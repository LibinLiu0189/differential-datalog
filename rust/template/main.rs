//! Test the generated Datalog program by reading Datalog commands from stdin (i.e., from a pipe or
//! CLI), parsing them with cmd_parser crate, executing commands, and tracking database state in a
//! map.

#![allow(non_snake_case, dead_code)]

//#![feature(alloc_system)]
//extern crate alloc_system;

extern crate datalog_example_ddlog;
extern crate differential_datalog;
extern crate cmd_parser;
extern crate time;

#[macro_use]
extern crate rustop;

use std::sync::Arc;
use std::sync::Mutex;
use std::process::exit;
use std::io;
use std::io::{Stdout,stdout,stderr};
use std::env;
//use std::mem;

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use datalog_example_ddlog::*;
use datalog_example_ddlog::valmap::*;
use datalog_example_ddlog::update_handler::*;
use differential_datalog::program::*;
use cmd_parser::*;
use differential_datalog::record::*;
use time::precise_time_ns;

// uncomment to enable profiling
//extern crate cpuprofiler;
//use cpuprofiler::PROFILER;

/*fn upd_cb(do_print: bool, do_store: bool, db: &Arc<Mutex<ValMap>>, relid: RelId, v: &Value, pol: bool) {
    if do_store {
        db.lock().unwrap().update(relid, v, pol);
    };
    if do_print {
        eprintln!("{} {:?} {}", if pol { "insert" } else { "delete" }, relid, *v);
    };
}*/

extern "C" fn print_fun(_arg: libc::uintptr_t,
                        table: libc::size_t,
                        rec: *const Record,
                        pol: bool) {
    unsafe {
        eprintln!("{} {:?} {}", if pol { "insert" } else { "delete" }, table, *rec);
    }
}

fn handle_cmd(db: &Arc<Mutex<ValMap>>, p: &mut RunningProgram<Value>, upds: &mut Vec<Update<Value>>, cmd: Command) -> (i32, bool) {
    let resp = (
        if !is_upd_cmd(&cmd) {
            apply_updates(p, upds)
        } else {
            Ok(())
        }).and(match cmd {
        Command::Start => {
            p.transaction_start()
        },
        Command::Commit => {
            // uncomment to enable profiling
            //PROFILER.lock().unwrap().start("./prof.profile").expect("Couldn't start profiling");
            let res = p.transaction_commit();
            //PROFILER.lock().unwrap().stop().expect("Couldn't stop profiler");
            res
        },
        Command::Comment => {
            Ok(())
        },
        Command::Rollback => {
            p.transaction_rollback()
        },
        Command::Timestamp => {
            println!("Timestamp: {}", precise_time_ns());
            Ok(())
        },
        Command::Profile(None) => {
            let profile = (*p.profile.lock().unwrap()).clone();
            println!("Profile:\n{}", profile);
            Ok(())
        },
        Command::Profile(Some(ProfileCmd::CPU(enable))) => {
            p.enable_cpu_profiling(enable);
            Ok(())
        },
        Command::Dump(None) => {
            db.lock().unwrap().format(&mut stdout());
            Ok(())
        },
        Command::Dump(Some(rname)) => {
            let relid = match output_relname_to_id(&rname) {
                None      => {
                    eprintln!("Error: Unknown output relation {}", rname);
                    return (-1, false);
                },
                Some(rid) => rid as RelId
            };
            db.lock().unwrap().format_rel(relid, &mut stdout());
            Ok(())
        },
        Command::Clear(rname) => {
            let relid = match input_relname_to_id(&rname) {
                None      => {
                    eprintln!("Error: Unknown input relation {}", rname);
                    return (-1, false);
                },
                Some(rid) => rid as RelId
            };
            p.clear_relation(relid);
            Ok(())
        },
        Command::Exit => {
            return (0, false);
        },
        Command::Echo(txt) => {
            println!("{}", txt);
            Ok(())
        },
        Command::Update(upd, last) => {
             match updcmd2upd(&upd) {
                Ok(u)  => upds.push(u),
                Err(e) => {
                    upds.clear();
                    eprintln!("Error: {}", e);
                    return (-1, false);
                }
            };
            if last {
                apply_updates(p, upds)
            } else {
                Ok(())
            }
        }
    });
    match resp {
        Ok(_)  => (0, true),
        Err(e) => {eprintln!("Error: {}", e); (-1, false)}
    }
}

fn apply_updates(p: &mut RunningProgram<Value>, upds: &mut Vec<Update<Value>>) -> Response<()> {
    let copy: Vec<Update<Value>> = upds.drain(..).collect();
    if copy.len() != 0 {
        p.apply_updates(copy)
    } else { Ok(()) }
}

fn is_upd_cmd(c: &Command) -> bool {
    match c {
        Command::Update(_,_) => true,
        _                    => false
    }
}

pub fn run_interactive(db: Arc<Mutex<ValMap>>, upd_cb: Box<dyn CBFn<Value>>, nworkers: usize) -> i32 {
    let p = prog(upd_cb);
    let mut running = Arc::new(Mutex::new(p.run(nworkers)));
    let upds = Arc::new(Mutex::new(Vec::new()));
    let ret = interact(|cmd| handle_cmd(&db.clone(), &mut running.lock().unwrap(), &mut upds.lock().unwrap(), cmd));
    Arc::try_unwrap(running).ok()
        .expect("run_interactive: cannot unwrap Arc")
        .into_inner()
        .expect("run_interactive: program is still locked")
        .stop();
    ret
}

pub fn main() {
    //println!("sizeof(Value) = {}", mem::size_of::<Value>());
    let parser = opts! {
        synopsis "DDlog CLI interface.";
        auto_shorts false;
        opt store:bool=true, desc:"Do not store relation state (for benchmarking only)."; // --no-store
        opt print:bool=true, desc:"Do not print deltas.";                                 // --no-print
        opt workers:usize=4, short:'w', desc:"The number of worker threads.";             // --workers or -w
    };
    let (args, rest) = parser.parse_or_exit();

    if rest.len() != 0 || args.workers == 0 {
        panic!("Invalid command line arguments; try -h for help");
    }

    let print   = args.print;
    let store   = args.store;
    let workers = args.workers;

    //let db: Arc<Mutex<ValMap>> = Arc::new(Mutex::new(ValMap::new()));

    let db: Arc<Mutex<ValMap>> = Arc::new(Mutex::new(ValMap::new()));
    let mut handlers: Vec<Box<dyn DynUpdateHandler<Value>>> = Vec::new();

    if store {
        handlers.push(Box::new(MTValMapUpdateHandler::new(db.clone())));
    };
    if print{
        handlers.push(Box::new(ExternCUpdateHandler::new(print_fun, 0)))
    };

    let handler: Box<dyn DynUpdateHandler<Value>> = if handlers.len() == 0 {
        Box::new(NullUpdateHandler::new())
    } else if handlers.len() == 1 {
        handlers[0].clone()
    } else {
        Box::new(ChainedDynUpdateHandler::new(handlers))
    };

    let handler2 = handler.clone();
    let ret = run_interactive(db.clone(), Box::new(move |relid,v,w| {
        debug_assert!(w == 1 || w == -1);
        handler2.update(relid, v, w == 1)
    }), workers);
    exit(ret);
}
