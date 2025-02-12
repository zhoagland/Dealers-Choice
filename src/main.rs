mod modules;

fn main() {
    println!("Hello, world!");
}


#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_main() {
        assert!(main() == ());
    }

    #[test]
    #[should_panic]
    fn test_main_fail() {
        assert!(main() != ());
    }
}