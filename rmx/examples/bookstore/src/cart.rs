//! `utils/cart.ts`: the cart kept in the session.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartItem {
    pub book_id: i64,
    pub slug: String,
    pub title: String,
    pub price: f64,
    pub quantity: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cart {
    pub items: Vec<CartItem>,
}

impl Cart {
    pub fn add(&mut self, book_id: i64, slug: &str, title: &str, price: f64, quantity: i64) {
        match self.items.iter_mut().find(|item| item.book_id == book_id) {
            Some(item) => item.quantity += quantity,
            None => self.items.push(CartItem { book_id, slug: slug.into(), title: title.into(), price, quantity }),
        }
    }

    /// Set a quantity; zero or less removes the item. `None` when the book is not in the cart.
    pub fn update(&mut self, book_id: i64, quantity: i64) -> Option<()> {
        let index = self.items.iter().position(|item| item.book_id == book_id)?;
        if quantity <= 0 {
            self.items.remove(index);
        } else {
            self.items[index].quantity = quantity;
        }
        Some(())
    }

    pub fn remove(&mut self, book_id: i64) {
        self.items.retain(|item| item.book_id != book_id);
    }

    pub fn contains(&self, book_id: i64) -> bool {
        self.items.iter().any(|item| item.book_id == book_id)
    }

    pub fn total(&self) -> f64 {
        self.items.iter().map(|item| item.price * item.quantity as f64).sum()
    }
}

/// `${value.toFixed(2)}`
pub fn money(value: f64) -> String {
    format!("${value:.2}")
}
