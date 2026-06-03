use std::io::IsTerminal;

pub struct Out;

impl Out {
    pub fn is_tty() -> bool {
        std::io::stdout().is_terminal()
    }

    pub fn println(msg: &str) {
        println!("{}", msg);
    }

    pub fn eprintln(msg: &str) {
        eprintln!("{}", msg);
    }
}
