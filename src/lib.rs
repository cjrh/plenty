// lib.rs
use std::io::{self, Write};
use std::collections::HashMap;
use std::error::Error;
use log::*;

#[derive(Debug, Clone)]
pub enum Token {
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
    Function(String), // Represents a function invocation like :add
    Open,
    ReadLines,
    Clear,
    ListDir,
}

impl std::str::FromStr for Token {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        debug!("Parsing token: {}", s);
        if s == "." {
            Ok(Token::Display)
        } else if s == "+" {
            Ok(Token::Plus)
        } else if s == "-" {
            Ok(Token::Minus)
        } else if s == "*" {
            Ok(Token::Multiply)
        } else if s == "/" {
            Ok(Token::Divide)
        } else if s == "(" {
            Ok(Token::LParen)
        } else if s == ")" {
            Ok(Token::RParen)
        } else if s == ":clear" {
            Ok(Token::Clear)
        } else if s == ":listdir" {
            Ok(Token::ListDir)
        } else if s.starts_with(':') {
            if s.len() == 1 {
                Err("Invalid function name")
            } else {
                Ok(Token::Function(s[1..].to_string()))
            }
        } else if let Ok(number) = s.parse::<i32>() {
            Ok(Token::NumberI32(number))
        } else {
            Ok(Token::Text(s.to_string()))
        }
    }
}

#[derive(Default)]
pub struct Stack {
    items: Vec<Token>,
    pub functions: HashMap<String, Vec<Token>>, // Stores function definitions
    literal_mode: bool,
}


impl Stack {
    pub fn new() -> Stack {
        Stack::default()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn repr(&self) -> String {
        format!("{:?}", self.items)
    }

    pub fn display(&self) {
        println!("{}", self.repr());
        println!("Functions: {:?}", self.functions);
    }

    pub fn push_str(&mut self, item: &str) -> Result<(), Box<dyn Error>> {
        if item.starts_with('`') && item.len() > 1 {
            // Single-token literal
            self.push(Token::Text(item[1..].to_string()))?;
        } else if item == "`" {
            // Enter literal mode
            self.literal_mode = true;
        } else if item == "~" && self.literal_mode {
            // Exit literal mode
            self.literal_mode = false;
        } else if self.literal_mode {
            // Add as literal in literal mode
            self.push(Token::Text(item.to_string()))?;
        } else {
            match item {
                ":make-fn" => self.make_function()?,
                _ => match item.parse::<Token>() {
                    Ok(token) => self.push(token)?,
                    Err(_) => self.push(Token::Text(item.to_string()))?,
                },
            }
        }
        Ok(())
    }

    pub fn push(&mut self, item: Token) -> Result<(), Box<dyn Error>> {
        match item {
            Token::Display => self.display(),
            Token::Plus => self.add()?,
            Token::Clear => self.clear(),
            Token::NumberI32(_) => self.items.push(item),
            Token::NumberI64(_) => self.items.push(item),
            Token::Text(_) => self.items.push(item),
            Token::Minus => self.subtract()?,
            Token::Multiply => self.multiply()?,
            Token::Divide => self.divide()?,
            Token::Function(name) => self.call_function(&name)?,
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

    fn call_function(&mut self, name: &str) -> Result<(), Box<dyn Error>> {
        debug!("Calling function: {}", name);
        if let Some(definition) = self.functions.get(name) {
            debug!("Function definition: {:?}", definition);
            for token in definition.clone() {
                let s = match token {
                    Token::Text(value) => value, // End marker
                    _ => return Err("Expected a text value".into()),
                };
                let operation = s.parse::<Token>()?;
                self.push(operation)?;
            }
            Ok(())
        } else {
            Err(format!("Undefined function: {}", name).into())
        }
    }

    fn make_function(&mut self) -> Result<(), Box<dyn Error>> {
        let mut tokens = vec![];

        // Collect tokens until the stack contains the function name
        while let Some(token) = self.pop() {
            match token {
                Token::Text(value) if value == "~" => break, // End marker
                Token::Text(value) => tokens.push(Token::Text(value)),
                _ => return Err("Expected a text value".into()),
            }
        }
        debug!("Function tokens: {:?}", tokens);

        if let Some(Token::Text(name)) = tokens.pop() {
            self.functions.insert(name, tokens);
            Ok(())
        } else {
            Err("Expected a function name".into())
        }
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

    fn subtract(&mut self) -> Result<(), Box<dyn Error>> {
        if let (Some(Token::NumberI32(b)), Some(Token::NumberI32(a))) = (self.pop(), self.pop()) {
            self.push(Token::NumberI32(a - b))?;
            Ok(())
        } else {
            Err("Expected two numbers".into())
        }
    }

    fn multiply(&mut self) -> Result<(), Box<dyn Error>> {
        if let (Some(Token::NumberI32(b)), Some(Token::NumberI32(a))) = (self.pop(), self.pop()) {
            self.push(Token::NumberI32(a * b))?;
            Ok(())
        } else {
            Err("Expected two numbers".into())
        }
    }

    fn divide(&mut self) -> Result<(), Box<dyn Error>> {
        if let (Some(Token::NumberI32(b)), Some(Token::NumberI32(a))) = (self.pop(), self.pop()) {
            if b == 0 {
                return Err("Division by zero".into());
            }
            self.push(Token::NumberI32(a / b))?;
            Ok(())
        } else {
            Err("Expected two numbers".into())
        }
    }

    fn pop(&mut self) -> Option<Token> {
        self.items.pop()
    }

    pub fn run_program(&mut self, program: &str) -> Result<Vec<String>, Box<dyn Error>> {
        let mut output = vec![];
        for line in program.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let items: Vec<&str> = line.split_whitespace().collect();
            for item in items {
                self.push_str(item)?;
            }
        }
        output.push(self.repr());
        Ok(output)
    }
}
