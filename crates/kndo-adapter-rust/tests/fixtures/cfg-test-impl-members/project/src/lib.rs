pub struct Widget {
    size: u32,
}

impl Widget {
    pub fn new(size: u32) -> Widget {
        Widget { size }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    #[cfg(test)]
    fn stub() -> Widget {
        Widget { size: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::Widget;

    #[test]
    fn stub_has_zero_size() {
        assert_eq!(Widget::stub().size(), 0);
    }

    #[test]
    fn new_keeps_the_given_size() {
        assert_eq!(Widget::new(3).size(), 3);
    }
}
