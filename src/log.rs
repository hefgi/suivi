use std::io::IsTerminal;

#[allow(dead_code)]
pub struct Out;

impl Out {
    #[allow(dead_code)]
    pub fn is_tty() -> bool {
        std::io::stdout().is_terminal()
    }

    #[allow(dead_code)]
    pub fn println(msg: &str) {
        println!("{}", msg);
    }

    #[allow(dead_code)]
    pub fn eprintln(msg: &str) {
        eprintln!("{}", msg);
    }
}
