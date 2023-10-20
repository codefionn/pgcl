use std::{hash::Hash, str::FromStr};

use bigdecimal::{
    num_bigint::ToBigInt, BigDecimal, FromPrimitive, One, ParseBigDecimalError, ToPrimitive, Zero,
};
use num::{BigInt, Integer};

use crate::errors::InterpreterError;

#[derive(Clone)]
pub struct BigRational {
    quotient: BigDecimal,
    fraction: BigDecimal,
}

impl BigRational {
    /// Create a new rational
    #[inline]
    pub fn new<T: Into<BigDecimal>, S: Into<BigDecimal>>(quotient: T, fraction: S) -> Self {
        let mut result = Self {
            quotient: quotient.into(),
            fraction: fraction.into(),
        };

        result.remove_floating_point();

        result
    }

    pub fn split(self) -> (BigInt, BigInt) {
        let res = self.reduce();

        (
            res.quotient.to_bigint().unwrap(),
            res.fraction.to_bigint().unwrap(),
        )
    }

    /// Reduce rational
    pub fn reduce(mut self) -> Self {
        self.quotient = self.quotient.normalized();
        self.fraction = self.fraction.normalized();
        self.remove_floating_point();

        let is_negative = self.quotient.sign() != self.fraction.sign();
        self.quotient = self.quotient.abs();
        self.fraction = self.fraction.abs();
        let a = self.quotient.to_bigint().unwrap();
        let b = self.fraction.to_bigint().unwrap();

        let gcd: BigDecimal = a.gcd(&b).into();

        self.quotient = BigDecimal::new(a, 0) / gcd.clone()
            * BigDecimal::from_i64(if is_negative { -1 } else { 1 }).unwrap();
        self.fraction = BigDecimal::new(b, 0) / gcd;

        self
    }

    pub fn abs(self) -> Self {
        let mut result = self.reduce();
        result.quotient = result.quotient.abs();
        result.fraction = result.fraction.abs();

        result
    }

    #[inline]
    fn remove_floating_point(&mut self) {
        let (_, quotient_pow) = self.quotient.as_bigint_and_exponent();
        let (_, fraction_pow) = self.fraction.as_bigint_and_exponent();

        let pow = quotient_pow.max(fraction_pow);
        if pow < 0 {
            return;
        }

        if pow == 0 {
            return;
        }

        let mul_pow: BigDecimal = BigInt::from_i64(10).unwrap().pow(pow as u32).into();
        self.quotient *= mul_pow.clone();
        self.fraction *= mul_pow;
    }
}

impl std::fmt::Debug for BigRational {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("{} / {}", self.quotient, self.fraction).as_str())
    }
}

impl FromStr for BigRational {
    type Err = ParseBigDecimalError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<BigDecimal>().map(|a| a.into())
    }
}

impl FromPrimitive for BigRational {
    fn from_i8(n: i8) -> Option<Self> {
        Some(Self::new(BigInt::from_i8(n)?, BigInt::from_u8(1)?))
    }

    fn from_u8(n: u8) -> Option<Self> {
        Some(Self::new(BigInt::from_u8(n)?, BigInt::from_u8(1)?))
    }

    fn from_i16(n: i16) -> Option<Self> {
        Some(Self::new(BigInt::from_i16(n)?, BigInt::from_u8(1)?))
    }

    fn from_u16(n: u16) -> Option<Self> {
        Some(Self::new(BigInt::from_u16(n)?, BigInt::from_u8(1)?))
    }

    fn from_i32(n: i32) -> Option<Self> {
        Some(Self::new(BigInt::from_i32(n)?, BigInt::from_u8(1)?))
    }

    fn from_u32(n: u32) -> Option<Self> {
        Some(Self::new(BigInt::from_u32(n)?, BigInt::from_u8(1)?))
    }

    fn from_i64(n: i64) -> Option<Self> {
        Some(Self::new(BigInt::from_i64(n)?, BigInt::from_u8(1)?))
    }

    fn from_u64(n: u64) -> Option<Self> {
        Some(Self::new(BigInt::from_u64(n)?, BigInt::from_u8(1)?))
    }

    fn from_i128(n: i128) -> Option<Self> {
        Some(Self::new(BigInt::from_i128(n)?, BigInt::from_u8(1)?))
    }

    fn from_u128(n: u128) -> Option<Self> {
        Some(Self::new(BigInt::from_u128(n)?, BigInt::from_u8(1)?))
    }

    fn from_usize(n: usize) -> Option<Self> {
        Some(Self::new(BigInt::from_usize(n)?, BigInt::from_u8(1)?))
    }

    fn from_f32(n: f32) -> Option<Self> {
        Some(Self::new(BigDecimal::from_f32(n)?, BigInt::from_u8(1)?))
    }

    fn from_f64(n: f64) -> Option<Self> {
        Some(Self::new(BigDecimal::from_f64(n)?, BigInt::from_u8(1)?))
    }
}

impl Zero for BigRational {
    fn zero() -> Self {
        Self::from_u8(0).unwrap()
    }

    fn is_zero(&self) -> bool {
        self.quotient.is_zero()
    }
}

impl One for BigRational {
    fn one() -> Self {
        Self::from_u8(1).unwrap()
    }

    fn is_one(&self) -> bool
    where
        Self: PartialEq,
    {
        let a = self.clone().reduce();

        a == Self::new(1, 1)
    }
}

impl From<BigDecimal> for BigRational {
    fn from(value: BigDecimal) -> Self {
        Self::new(value, BigInt::from_u8(1).unwrap())
    }
}

impl From<BigInt> for BigRational {
    fn from(value: BigInt) -> Self {
        Self::new(value, BigInt::from_u8(1).unwrap())
    }
}

impl From<BigRational> for BigDecimal {
    fn from(value: BigRational) -> Self {
        value.quotient / value.fraction
    }
}

impl TryFrom<BigRational> for f64 {
    type Error = InterpreterError;

    fn try_from(value: BigRational) -> Result<Self, Self::Error> {
        (value.quotient / value.fraction)
            .to_f64()
            .ok_or_else(|| InterpreterError::NumberTooBig())
    }
}

impl Hash for BigRational {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.quotient.hash(state);
        self.fraction.hash(state);
    }
}

impl Eq for BigRational {}

impl PartialEq for BigRational {
    fn eq(&self, other: &Self) -> bool {
        let a = self.clone().reduce();
        let b = other.clone().reduce();

        a.quotient == b.quotient && a.fraction == b.fraction
    }
}

impl PartialOrd for BigRational {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let a: BigDecimal = self.clone().into();
        let b: BigDecimal = other.clone().into();

        a.partial_cmp(&b)
    }
}

impl PartialEq<BigDecimal> for BigRational {
    fn eq(&self, other: &BigDecimal) -> bool {
        let a = self.clone().reduce();
        let b: BigRational = other.clone().into();
        let b = b.reduce();

        a == b
    }
}

impl PartialEq<BigInt> for BigRational {
    fn eq(&self, other: &BigInt) -> bool {
        let a = self.clone().reduce();
        let b: BigRational = other.clone().into();

        a == b
    }
}

impl std::ops::Add for BigRational {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            quotient: self.quotient * rhs.fraction.clone() + rhs.quotient * self.fraction.clone(),
            fraction: self.fraction * rhs.fraction,
        }
    }
}

impl std::ops::Sub for BigRational {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            quotient: self.quotient * rhs.fraction.clone() - rhs.quotient * self.fraction.clone(),
            fraction: self.fraction * rhs.fraction,
        }
    }
}

impl std::ops::Mul for BigRational {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            quotient: self.quotient * rhs.quotient,
            fraction: self.fraction * rhs.fraction,
        }
    }
}

impl std::ops::Div for BigRational {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        Self {
            quotient: self.quotient * rhs.fraction,
            fraction: self.fraction * rhs.quotient,
        }
    }
}
