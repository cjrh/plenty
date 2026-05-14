//! The Plenty REPL.

use std::io::{self, Write};

use plenty::Vm;

const BANNER: &str = r#"
:::::::::  :::        :::::::::: ::::    ::: ::::::::::: :::   :::
:+:    :+: :+:        :+:        :+:+:   :+:     :+:     :+:   :+:
+:+    +:+ +:+        +:+        :+:+:+  +:+     +:+      +:+ +:+
+#++:++#+  +#+        +#++:++#   +#+ +:+ +#+     +#+       +#++:
+#+        +#+        +#+        +#+  +#+#+#     +#+        +#+
#+#        #+#        #+#        #+#   #+#+#     #+#        #+#
###        ########## ########## ###    ####     ###        ###
"#;

fn main() {
    pretty_env_logger::init();
    println!("{BANNER}");

    let mut vm = Vm::new();
    let stdin = io::stdin();
    let mut line = String::new();

    loop {
        print!("---> ");
        io::stdout().flush().expect("stdout flush failed");

        line.clear();
        // `read_line` returns Ok(0) only at end of input (e.g. Ctrl-D).
        if stdin.read_line(&mut line).expect("stdin read failed") == 0 {
            println!();
            break;
        }

        let source = line.trim();
        if matches!(source, "exit" | "q" | "quit") {
            break;
        }

        if let Err(e) = vm.run(source) {
            eprintln!("error: {e}");
        }
    }
}
