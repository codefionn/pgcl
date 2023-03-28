use bigdecimal::{BigDecimal, FromPrimitive};

use crate::rational::BigRational;

#[test]
fn test_reduce() {
    assert_eq!(
        BigRational::new(10, 10).reduce(),
        BigRational::from_u8(1).unwrap()
    );
    assert_eq!(
        BigRational::new(100, 10).reduce(),
        BigRational::from_u8(10).unwrap()
    );
    assert_eq!(
        BigRational::new(BigDecimal::from_f64(10.8).unwrap(), 10).reduce(),
        BigRational::new(27, 25)
    );
}
