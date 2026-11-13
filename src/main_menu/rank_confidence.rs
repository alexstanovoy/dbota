//! A display index from published per-mode uncertainty, not a probability or a new rating rule.
const INITIAL_DEVIATION:f64=350.0;
const INITIAL_VOLATILITY:f64=0.06;
const GLICKO_SCALE:f64=173.7178;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Confidence {pub percent:u8,pub calibrated:bool}
impl Confidence {
    pub const NEW:Self=Self{percent:0,calibrated:false};
}

/// Equal-rating/equal-deviation steady state at the initial volatility. Individual volatility
/// evolves, so this is a nominal active reference, never an invariant positive Glicko floor.
fn active_reference()->f64 {
    let sigma_squared=INITIAL_VOLATILITY*INITIAL_VOLATILITY;
    let coefficient=sigma_squared*(1.0-12.0/(std::f64::consts::PI*std::f64::consts::PI));
    let variance=(-coefficient+(coefficient*coefficient+16.0*sigma_squared).sqrt())/2.0;
    GLICKO_SCALE*variance.sqrt()
}

pub fn from_deviation(deviation:f64,games:u32)->Option<Confidence> {
    if !deviation.is_finite() || deviation<0.0 {return None;}
    let percent=(100.0*(INITIAL_DEVIATION-deviation)/(INITIAL_DEVIATION-active_reference())).clamp(0.0,100.0).round() as u8;
    // The public ladder lists played accounts; it has no deviation-based calibration cutoff.
    Some(Confidence{percent,calibrated:games>0})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_display_reference_uses_the_published_rating_defaults() {
        assert_eq!(INITIAL_DEVIATION,radovid::rating::INITIAL_DEVIATION);
        assert_eq!(INITIAL_VOLATILITY,radovid::rating::INITIAL_VOLATILITY);
        assert!((active_reference()-60.27511181392085).abs()<1e-10);
        assert_eq!(from_deviation(350.0,0),Some(Confidence::NEW));
    }
    #[test]
    fn confidence_is_monotone_and_invalid_uncertainty_is_absent() {
        let values=[400.0,350.0,290.31896,200.0,100.0,60.27511181392085,0.0];
        let percentages:Vec<_>=values.map(|value|from_deviation(value,1).expect("finite deviation").percent).into();
        assert!(percentages.windows(2).all(|pair|pair[0]<=pair[1]));
        assert_eq!(percentages[0],0);
        assert_eq!(percentages[2],21);
        assert_eq!(percentages[5],100);
        for bad in [f64::NAN,f64::INFINITY,-1.0] {assert_eq!(from_deviation(bad,1),None);}
    }
}
