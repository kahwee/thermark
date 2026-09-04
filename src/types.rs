//! Shared domain newtypes: density, rotation, threshold.

use crate::errors::{Error, Result};
use std::fmt;
use std::str::FromStr;

/// Print density level **1..=5** (device protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Density(u8);

impl Density {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 5;
    /// Default for general prints.
    pub const NORMAL: Self = Self(3);
    /// Slightly darker — useful for calibration / small QR text.
    pub const DARK: Self = Self(4);

    /// Validated constructor.
    pub fn new(value: u8) -> Result<Self> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(Error::InvalidDensity(value))
        }
    }

    /// Saturating clamp into 1..=5 (for untrusted inputs that must not fail).
    pub fn clamp(value: u8) -> Self {
        Self(value.clamp(Self::MIN, Self::MAX))
    }

    #[inline]
    pub fn get(self) -> u8 {
        self.0
    }
}

impl Default for Density {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl fmt::Display for Density {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Density {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let v: u8 = s
            .trim()
            .parse()
            .map_err(|_| Error::msg(format!("invalid density '{s}' (need 1..=5)")))?;
        Self::new(v)
    }
}

impl TryFrom<u8> for Density {
    type Error = Error;
    fn try_from(value: u8) -> Result<Self> {
        Self::new(value)
    }
}

impl From<Density> for u8 {
    fn from(d: Density) -> u8 {
        d.0
    }
}

/// Clockwise image rotation for print jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Rotation {
    /// Parse degrees (any multiple of 360 collapses to 0).
    pub fn from_degrees(deg: u32) -> Result<Self> {
        match deg % 360 {
            0 => Ok(Self::Deg0),
            90 => Ok(Self::Deg90),
            180 => Ok(Self::Deg180),
            270 => Ok(Self::Deg270),
            other => Err(Error::InvalidRotation(other)),
        }
    }

    #[inline]
    pub fn degrees(self) -> u32 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }

    /// True when no rotation is applied.
    pub fn is_identity(self) -> bool {
        matches!(self, Self::Deg0)
    }
}

impl fmt::Display for Rotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.degrees())
    }
}

impl FromStr for Rotation {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let deg: u32 = s
            .trim()
            .parse()
            .map_err(|_| Error::msg(format!("invalid rotation '{s}' (use 0/90/180/270)")))?;
        Self::from_degrees(deg)
    }
}

/// Grayscale → 1-bit threshold after invert (0–255).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Threshold(u8);

impl Threshold {
    pub const DEFAULT: Self = Self(127);

    #[inline]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    #[inline]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl Default for Threshold {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for Threshold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Threshold {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let v: u8 = s
            .trim()
            .parse()
            .map_err(|_| Error::msg(format!("invalid threshold '{s}' (need 0..=255)")))?;
        Ok(Self::new(v))
    }
}

impl From<u8> for Threshold {
    fn from(v: u8) -> Self {
        Self(v)
    }
}

impl From<Threshold> for u8 {
    fn from(t: Threshold) -> u8 {
        t.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_valid_and_invalid() {
        assert_eq!(Density::new(1).unwrap().get(), 1);
        assert_eq!(Density::new(5).unwrap().get(), 5);
        assert!(Density::new(0).is_err());
        assert!(Density::new(6).is_err());
        assert_eq!(Density::clamp(0).get(), 1);
        assert_eq!(Density::clamp(9).get(), 5);
        assert_eq!("4".parse::<Density>().unwrap(), Density::DARK);
    }

    #[test]
    fn rotation_degrees() {
        assert_eq!(Rotation::from_degrees(0).unwrap(), Rotation::Deg0);
        assert_eq!(Rotation::from_degrees(360).unwrap(), Rotation::Deg0);
        assert_eq!(Rotation::from_degrees(90).unwrap(), Rotation::Deg90);
        assert!(Rotation::from_degrees(45).is_err());
        assert_eq!(Rotation::Deg270.degrees(), 270);
    }

    #[test]
    fn threshold_default() {
        assert_eq!(Threshold::DEFAULT.get(), 127);
        assert_eq!("200".parse::<Threshold>().unwrap().get(), 200);
    }
}
