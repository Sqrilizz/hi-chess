#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    GameStart,
    Thinking,
    Brilliant,
    Best,
    Good,
    Inaccuracy,
    Mistake,
    Blunder,
    Victory,
    Defeat,
    Draw,
}

impl std::str::FromStr for Moment {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "game-start" => Ok(Self::GameStart),
            "thinking" => Ok(Self::Thinking),
            "brilliant" => Ok(Self::Brilliant),
            "best" => Ok(Self::Best),
            "good" => Ok(Self::Good),
            "inaccuracy" => Ok(Self::Inaccuracy),
            "mistake" => Ok(Self::Mistake),
            "blunder" => Ok(Self::Blunder),
            "victory" => Ok(Self::Victory),
            "defeat" => Ok(Self::Defeat),
            "draw" => Ok(Self::Draw),
            _ => Err("unknown personality moment".to_string()),
        }
    }
}

pub fn line(moment: Moment, variation: usize) -> &'static str {
    let lines: &[&str] = match moment {
        Moment::GameStart => &[
            "Ну что, посмотрим, как ты играешь.",
            "Партия началась. Постарайся не скучать.",
            "Я готов. А ты?",
            "Сегодня попробуем без глупых ошибок.",
        ],
        Moment::Thinking => &[
            "Думаю. Это может занять несколько секунд.",
            "Сейчас найду что-нибудь интересное.",
            "Не торопи меня. Хороший ход требует времени.",
            "Секунду, просчитываю варианты.",
        ],
        Moment::Brilliant => &[
            "Вот это ход. Красиво.",
            "Я такого не ожидал. Отличная находка.",
            "Сильно. Придётся теперь серьёзно поработать.",
            "Вот ради таких моментов я и играю.",
        ],
        Moment::Best => &[
            "Лучший ход на доске.",
            "Точно. Так держать.",
            "Хороший выбор.",
            "С этим вариантом трудно спорить.",
        ],
        Moment::Good => &[
            "Разумно.",
            "Можно было и лучше, но игра продолжается.",
            "Нормальный ход.",
            "Приемлемо. Идём дальше.",
        ],
        Moment::Inaccuracy => &[
            "Немного неточно. Посмотрим, что будет дальше.",
            "Есть вариант получше.",
            "Маленькая неаккуратность.",
            "Не критично, но мне это нравится меньше.",
        ],
        Moment::Mistake => &[
            "Вот здесь можно было поиграть точнее.",
            "Ошибка. Но ещё не всё потеряно.",
            "Слишком много отдал без необходимости.",
            "После этого хода позиция стала заметно хуже.",
        ],
        Moment::Blunder => &[
            "Ой. Это была серьёзная ошибка.",
            "Такой ход я бы не рекомендовал.",
            "Здесь можно было потерять партию. Почти потерял.",
            "Мда. Давай исправимся на следующем ходу.",
        ],
        Moment::Victory => &[
            "Хорошая партия. Спасибо за игру.",
            "Победа. В следующий раз будет сложнее.",
            "Я доволен. Надеюсь, ты тоже получил удовольствие.",
            "Мат. Зато было интересно.",
        ],
        Moment::Defeat => &[
            "Ты победил. Хорошая игра.",
            "Признаю, сегодня ты был сильнее.",
            "Вот это мат. Реванш?",
            "Мне есть чему поучиться. Хорошая партия.",
        ],
        Moment::Draw => &[
            "Ничья. Оба немного устали ошибаться.",
            "Равная игра. Принимаю.",
            "Ничья — честный результат.",
            "Сегодня без победителя. Реванш?",
        ],
    };
    lines[variation % lines.len()]
}

#[cfg(test)]
mod tests {
    use super::{Moment, line};

    #[test]
    fn parses_moment_names() {
        assert_eq!("blunder".parse::<Moment>(), Ok(Moment::Blunder));
        assert!("unknown".parse::<Moment>().is_err());
    }

    #[test]
    fn every_moment_has_nonempty_lines() {
        for moment in [
            Moment::GameStart,
            Moment::Thinking,
            Moment::Brilliant,
            Moment::Best,
            Moment::Good,
            Moment::Inaccuracy,
            Moment::Mistake,
            Moment::Blunder,
            Moment::Victory,
            Moment::Defeat,
            Moment::Draw,
        ] {
            for variation in 0..8 {
                assert!(!line(moment, variation).is_empty());
            }
        }
    }
}
