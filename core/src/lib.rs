use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Language {
    Dutch,
    English,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub id: Uuid,
    pub text: String,
    pub translation: Option<String>,
    pub chapter: Option<String>,
    pub group: Option<String>,
    pub language: Language,
    pub sentence: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: Uuid,
    pub word_id: Uuid,
    pub due_at: DateTime<Utc>,
    pub interval_days: i32,
    pub ease: f64,
    pub reps: i32,
    pub lapses: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: Uuid,
    pub card_id: Uuid,
    pub grade: u8,
    pub reviewed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub max_cards: usize,
    pub max_new_cards: usize,
    pub stop_after_correct: usize,
    pub max_minutes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub base_url: String,
    pub auth_token: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_cards: 20,
            max_new_cards: 10,
            stop_after_correct: 15,
            max_minutes: None,
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.example.com".to_string(),
            auth_token: String::new(),
        }
    }
}

pub fn default_new_card(word_id: Uuid, now: DateTime<Utc>) -> Card {
    Card {
        id: Uuid::new_v4(),
        word_id,
        due_at: now,
        interval_days: 0,
        ease: 2.5,
        reps: 0,
        lapses: 0,
    }
}

pub fn schedule_sm2(card: &mut Card, grade: u8, now: DateTime<Utc>) -> DateTime<Utc> {
    let clamped = grade.min(5);
    let quality = clamped as f32;

    let ease_delta = 0.1 - (5.0 - quality) * (0.08 + (5.0 - quality) * 0.02);
    card.ease = (card.ease + ease_delta as f64).max(1.3);

    if clamped < 3 {
        card.reps = 0;
        card.lapses += 1;
        card.interval_days = 1;
    } else {
        card.reps += 1;
        card.interval_days = match card.reps {
            1 => 1,
            2 => 6,
            _ => ((card.interval_days as f64) * card.ease).round() as i32,
        };
    }

    card.due_at = now + Duration::days(card.interval_days.max(1).into());
    card.due_at
}
