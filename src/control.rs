use std::time::{Duration, Instant};
use win_nightlight_lib::{ColorTemperature, Nightlight};

const MIN_KELVIN: i32 = 1200;
const MAX_KELVIN: i32 = 6500;
pub const MAX_STRENGTH_TENTHS: i32 = 1000;

fn kelvin_for(strength_tenths: i32) -> u16 {
    (MAX_KELVIN
        - strength_tenths.clamp(0, MAX_STRENGTH_TENTHS) * (MAX_KELVIN - MIN_KELVIN)
            / MAX_STRENGTH_TENTHS) as u16
}

fn tenths_for(kelvin: i16) -> i32 {
    let range = MAX_KELVIN - MIN_KELVIN;
    ((MAX_KELVIN - i32::from(kelvin)) * MAX_STRENGTH_TENTHS + range / 2) / range
}

pub struct Controller {
    nightlight: Nightlight,
    pub strength_tenths: i32,
    pub active: bool,
    preview: bool,
    pending_write: bool,
    last_write: Option<Instant>,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            nightlight: Nightlight::new(),
            strength_tenths: 500,
            active: false,
            preview: false,
            pending_write: false,
            last_write: None,
        }
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        let state = self.nightlight.read_state().map_err(|e| e.to_string())?;
        if !state.is_usable() {
            return Err("Night Light is unavailable on this display".into());
        }
        let settings = self.nightlight.read_settings().map_err(|e| e.to_string())?;
        self.active = state.is_active();
        if let Some(kelvin) = settings.color_temperature() {
            self.strength_tenths = tenths_for(kelvin).clamp(0, MAX_STRENGTH_TENTHS);
        }
        Ok(())
    }

    pub fn set_strength(
        &mut self,
        strength_tenths: i32,
        dragging: bool,
        force: bool,
    ) -> Result<(), String> {
        let strength_tenths = strength_tenths.clamp(0, MAX_STRENGTH_TENTHS);
        if strength_tenths == self.strength_tenths && self.active && !force {
            return Ok(());
        }
        if !self.active {
            self.nightlight
                .set_active(true)
                .map_err(|e| e.to_string())?;
            self.active = true;
        }
        let temperature =
            ColorTemperature::new(kelvin_for(strength_tenths)).map_err(|e| e.to_string())?;
        if dragging {
            if !self.preview {
                self.nightlight.start_preview().map_err(|e| e.to_string())?;
                self.preview = true;
            }
            if force
                || self
                    .last_write
                    .is_none_or(|time| time.elapsed() >= Duration::from_millis(33))
            {
                self.nightlight
                    .preview_color_temperature(temperature)
                    .map_err(|e| e.to_string())?;
                self.last_write = Some(Instant::now());
                self.pending_write = false;
            } else {
                self.pending_write = true;
            }
        } else {
            self.nightlight
                .apply_color_temperature(temperature)
                .map_err(|e| e.to_string())?;
        }
        self.strength_tenths = strength_tenths;
        Ok(())
    }

    pub fn finish_drag(&mut self) -> Result<(), String> {
        if self.preview {
            let pending_error = if self.pending_write {
                ColorTemperature::new(kelvin_for(self.strength_tenths))
                    .and_then(|temperature| self.nightlight.preview_color_temperature(temperature))
                    .err()
                    .map(|error| error.to_string())
            } else {
                None
            };
            let mut last_error = None;
            for _ in 0..3 {
                match self.nightlight.end_preview() {
                    Ok(_) => {
                        self.preview = false;
                        self.pending_write = false;
                        self.last_write = None;
                        return pending_error.map_or(Ok(()), Err);
                    }
                    Err(error) => last_error = Some(error.to_string()),
                }
            }
            return Err(last_error.unwrap_or_else(|| "Could not end Night Light preview".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenth_percent_steps_change_temperature() {
        assert_eq!(kelvin_for(0), 6500);
        assert_eq!(kelvin_for(1000), 1200);
        assert!(kelvin_for(501) < kelvin_for(500));
        assert_eq!(kelvin_for(10), 6447);
        for strength in 0..=MAX_STRENGTH_TENTHS {
            assert_eq!(tenths_for(kelvin_for(strength) as i16), strength);
        }
    }
}
