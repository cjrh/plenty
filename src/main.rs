use std::io::Write;

// main.rs
fn main() {
    let banner = r#"
:::::::::  :::        :::::::::: ::::    ::: ::::::::::: :::   :::
:+:    :+: :+:        :+:        :+:+:   :+:     :+:     :+:   :+:
+:+    +:+ +:+        +:+        :+:+:+  +:+     +:+      +:+ +:+
+#++:++#+  +#+        +#++:++#   +#+ +:+ +#+     +#+       +#++:
+#+        +#+        +#+        +#+  +#+#+#     +#+        +#+
#+#        #+#        #+#        #+#   #+#+#     #+#        #+#
###        ########## ########## ###    ####     ###        ###
"#;
    println!("{}", banner);

    let mut stack = plenty::Stack::new();

    // Let's make an interpreter loop
    let prompt = "---> ";
    let mut input = String::new();
    loop {
        print!("{}", prompt);
        std::io::stdout().flush().unwrap();

        input.clear();
        std::io::stdin().read_line(&mut input).unwrap();
        input.shrink_to_fit();
        let input_clean = &input[..input.len()-1].trim();

        if ["exit", "q", "quit"].contains(input_clean) {
            break;
        }

        let items: Vec<&str> = input_clean.split_whitespace().collect();
        for item in items {
            match stack.run_program(item) {
                Ok(_) => {
                    // println!("{:?}", stack.repr());
                },
                Err(e) => {
                    eprintln!("Error: {}", e);
                    break;
                }
            }
        }
    }
}
