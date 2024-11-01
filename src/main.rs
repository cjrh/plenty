use std::io::Write;
use std::collections::HashMap;
use std::error::Error;

#[derive(Debug, Clone)]
enum Token {
    Display,
    NumberI32(i32),
    NumberI64(i64),
    Text(String),
    MakeArrayNumberI32,
    MakeArrayText,
    ArrayNumberI32(Vec<i32>),
    ArrayText(Vec<String>),
    Join,
    Plus,
    Minus,
    Multiply,
    Divide,
    LParen,
    RParen,
    Open,
    ReadLines,
    Clear,
    ListDir,
}

impl std::str::FromStr for Token {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "." => Ok(Token::Display),
            "+" => Ok(Token::Plus),
            "-" => Ok(Token::Minus),
            "*" => Ok(Token::Multiply),
            "/" => Ok(Token::Divide),
            "(" => Ok(Token::LParen),
            ")" => Ok(Token::RParen),

            "open" => Ok(Token::Open),
            "readlines" => Ok(Token::ReadLines),
            "clear" => Ok(Token::Clear),
            "listdir" => Ok(Token::ListDir),

            "arrnum" => Ok(Token::MakeArrayNumberI32),
            "arrtxt" => Ok(Token::MakeArrayText),
            "join" => Ok(Token::Join),
            _ => {
                if let Ok(number) = s.parse::<i32>() {
                    Ok(Token::NumberI32(number))
                } else {
                    Ok(Token::Text(s.to_string()))
                }
            }
        }
    }
}

struct Stack {
    items: Vec<Token>,
}

impl Stack {
    fn new() -> Stack {
        Stack {
            items: vec![],
        }
    }

    fn clear(&mut self) {
        self.items.clear();
    }

    fn display(&self) {
        println!("{:?}", self.items);
    }

    fn push_str(&mut self, item: &str) -> Result<(), Box<dyn Error>> {
        match item.parse::<Token>() {
            Ok(token) => {
                self.push(token)?;
            },
            Err(_) => {
                self.push(Token::Text(item.to_string()))?;
            }
        }
        Ok(())
    }

    fn push(&mut self, item: Token) -> Result<(), Box<dyn Error>> {
        match item {
            Token::Display => self.display(),
            Token::Plus => self.add()?,
            Token::Clear => self.clear(),
            Token::NumberI32(_) => self.items.push(item),
            Token::NumberI64(_) => self.items.push(item),
            Token::Text(_) => self.items.push(item),
            Token::Minus => todo!(),
            Token::Multiply => self.multiply()?,
            Token::Divide => todo!(),
            Token::LParen => todo!(),
            Token::RParen => todo!(),
            Token::Open => todo!(),
            Token::ReadLines => todo!(),
            Token::ListDir => self.list_dir()?,
            Token::MakeArrayNumberI32 => self.array_number_i32()?,
            Token::MakeArrayText => self.array_text()?,
            Token::ArrayNumberI32(_) => self.items.push(item),
            Token::ArrayText(_) => self.items.push(item),
            Token::Join => self.join()?,
        }
        Ok(())
    }

    fn join(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(Token::ArrayText(items)) = self.pop() {
            let total = items.join("");
            self.push(Token::Text(total))?;
        } else {
            return Err("Expected an array of text".into());
        }
        Ok(())
    }

    fn array_number_i32(&mut self) -> Result<(), Box<dyn Error>> {
        let mut items = vec![];
        let count = match self.pop() {
            Some(Token::NumberI32(count)) => count,
            _ => return Err("Expected a number".into()),
        };

        for _ in 0..count {
            if let Some(Token::NumberI32(item)) = self.pop() {
                items.push(item);
            }
        }

        self.push(Token::ArrayNumberI32(items))?;
        Ok(())
    }

    fn array_text(&mut self) -> Result<(), Box<dyn Error>> {
        let mut items = vec![];
        let count = match self.pop() {
            Some(Token::NumberI32(count)) => count,
            _ => return Err("Expected a number".into()),
        };

        for _ in 0..count {
            if let Some(Token::Text(item)) = self.pop() {
                items.push(item);
            }
        }

        self.push(Token::ArrayText(items))?;
        Ok(())
    }


    fn list_dir(&self) -> Result<(), Box<dyn Error>> {
        let dir = std::fs::read_dir(".")?;
        for entry in dir {
            let entry = entry?;
            let path = entry.path();
            let path_str = path.to_str().unwrap();
            println!("{}", path_str);
        }
        Ok(())
    }

    fn add(&mut self) -> Result<(), Box<dyn Error>> {
        if self.items.len() < 2 {
            return Ok(());
        }

        let token_a = self.pop().unwrap();
        let token_b = self.pop().unwrap();
        match (&token_a, &token_b) {
            (Token::NumberI32(a), Token::NumberI32(b)) => {
                self.push(Token::NumberI32(a + b))?;
            },
            (Token::Text(a), Token::Text(b)) => {
                self.push(Token::Text(format!("{}{}", a, b)))?;
            },
            _ => {
                return Err(format!("Cannot add {:?} to {:?}", &token_a, &token_b).into());
            }
        }
        Ok(())
    }

    fn multiply(&mut self) -> Result<(), Box<dyn Error>> {
        if self.items.len() < 2 {
            return Ok(());
        }

        if let Some(Token::NumberI32(b)) = self.pop() {
            if let Some(Token::NumberI32(a)) = self.pop() {
                self.push(Token::NumberI32(a * b))?;
            } else {
                return Err("Expected a number".into());
            }
            Ok(())
        } else {
            Err("Expected a number".into())
        }
    }

    fn pop(&mut self) -> Option<Token> {
        self.items.pop()
    }
}


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

    let mut stack = Stack::new();

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
            match stack.push_str(item) {
                Ok(_) => {},
                Err(e) => {
                    eprintln!("Error: {}", e);
                    break;
                }
            }
        }
    }
}
