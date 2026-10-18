use crate::rhoapi::cost_signature_atom::Form;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{
    Bundle, Connective, CostRegion, CostSignature, CostTokenStack, Expr, If, Match, New, Receive,
    Send,
};

pub struct Score;

pub const NONSEMANTIC_SCORE_CONSTRUCTOR: i32 = i32::MAX;

impl Score {
    pub const ABSENT: i32 = 0;
    pub const BOOL: i32 = 1;
    pub const INT: i32 = 2;
    pub const STRING: i32 = 3;
    pub const URI: i32 = 4;
    pub const PRIVATE: i32 = 5;
    pub const ELIST: i32 = 6;
    pub const ETUPLE: i32 = 7;
    pub const ESET: i32 = 8;
    pub const EMAP: i32 = 9;
    pub const DEPLOYER_AUTH: i32 = 10;
    pub const DEPLOY_ID: i32 = 11;
    pub const SYS_AUTH_TOKEN: i32 = 12;
    pub const EPATHMAP: i32 = 13;
    pub const DOUBLE: i32 = 14;
    pub const BIG_INT: i32 = 15;
    pub const BIG_RAT: i32 = 16;
    pub const FIXED_POINT: i32 = 17;
    pub const BOUND_VAR: i32 = 50;
    pub const FREE_VAR: i32 = 51;
    pub const WILDCARD: i32 = 52;
    pub const REMAINDER: i32 = 53;
    pub const EVAR: i32 = 100;
    pub const ENEG: i32 = 101;
    pub const EMULT: i32 = 102;
    pub const EDIV: i32 = 103;
    pub const EPLUS: i32 = 104;
    pub const EMINUS: i32 = 105;
    pub const ELT: i32 = 106;
    pub const ELTE: i32 = 107;
    pub const EGT: i32 = 108;
    pub const EGTE: i32 = 109;
    pub const EEQ: i32 = 110;
    pub const ENEQ: i32 = 111;
    pub const ENOT: i32 = 112;
    pub const EAND: i32 = 113;
    pub const EOR: i32 = 114;
    pub const EMETHOD: i32 = 115;
    pub const EBYTEARR: i32 = 116;
    pub const EEVAL: i32 = 117;
    pub const EMATCHES: i32 = 118;
    pub const EPERCENT: i32 = 119;
    pub const EPLUSPLUS: i32 = 120;
    pub const EMINUSMINUS: i32 = 121;
    pub const EMOD: i32 = 122;
    pub const QUOTE: i32 = 203;
    pub const CHAN_VAR: i32 = 204;
    pub const SEND: i32 = 300;
    pub const RECEIVE: i32 = 301;
    pub const NEW: i32 = 303;
    pub const MATCH: i32 = 304;
    pub const BUNDLE_EQUIV: i32 = 305;
    pub const BUNDLE_READ: i32 = 306;
    pub const BUNDLE_WRITE: i32 = 307;
    pub const BUNDLE_READ_WRITE: i32 = 308;
    pub const IF: i32 = 309;
    pub const CONNECTIVE_NOT: i32 = 400;
    pub const CONNECTIVE_AND: i32 = 401;
    pub const CONNECTIVE_OR: i32 = 402;
    pub const CONNECTIVE_VARREF: i32 = 403;
    pub const CONNECTIVE_BOOL: i32 = 404;
    pub const CONNECTIVE_INT: i32 = 405;
    pub const CONNECTIVE_STRING: i32 = 406;
    pub const CONNECTIVE_URI: i32 = 407;
    pub const CONNECTIVE_BYTEARRAY: i32 = 408;
    pub const COST_REGION: i32 = 500;
    pub const COST_SIGNATURE: i32 = 501;
    pub const COST_GROUND: i32 = 502;
    pub const COST_QUOTE: i32 = 503;
    pub const COST_BOUND: i32 = 504;
    pub const COST_FORM_ABSENT: i32 = 505;
    pub const COST_SIGNER_ABSENT: i32 = 506;
    pub const COST_RUNTIME_NAME: i32 = 507;
    pub const COST_TOKEN_STACK: i32 = 508;
    pub const PAR: i32 = 999;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ParComponentTag {
    Send = 1,
    Receive = 2,
    New = 4,
    Expr = 5,
    Match = 6,
    GUnforgeable = 7,
    Connective = 8,
    Bundle = 11,
    If = 12,
    CostTokenStack = 13,
}

impl ParComponentTag {
    pub const fn byte(self) -> u8 { self as u8 }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Send => 0,
            Self::Receive => 1,
            Self::New => 2,
            Self::Expr => 3,
            Self::Match => 4,
            Self::GUnforgeable => 5,
            Self::Connective => 6,
            Self::Bundle => 7,
            Self::If => 8,
            Self::CostTokenStack => 9,
        }
    }

    pub(crate) const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Send),
            2 => Some(Self::Receive),
            4 => Some(Self::New),
            5 => Some(Self::Expr),
            6 => Some(Self::Match),
            7 => Some(Self::GUnforgeable),
            8 => Some(Self::Connective),
            11 => Some(Self::Bundle),
            12 => Some(Self::If),
            13 => Some(Self::CostTokenStack),
            _ => None,
        }
    }
}

pub const PAR_SCORE_TAGS: [ParComponentTag; 10] = [
    ParComponentTag::Send,
    ParComponentTag::Receive,
    ParComponentTag::Expr,
    ParComponentTag::New,
    ParComponentTag::Match,
    ParComponentTag::Bundle,
    ParComponentTag::Connective,
    ParComponentTag::GUnforgeable,
    ParComponentTag::If,
    ParComponentTag::CostTokenStack,
];

pub trait CanonicalScoreAlgebra {
    type Score;
    type Error;

    fn integer(&mut self, value: i64) -> Result<Self::Score, Self::Error>;
    fn string(&mut self, value: &str) -> Result<Self::Score, Self::Error>;
    fn bytes(&mut self, value: &[u8]) -> Result<Self::Score, Self::Error>;
    fn node(&mut self, children: Vec<Self::Score>) -> Result<Self::Score, Self::Error>;

    fn constructor(
        &mut self,
        constructor: i32,
        children: Vec<Self::Score>,
    ) -> Result<Self::Score, Self::Error> {
        let constructor = self.integer(i64::from(constructor))?;
        let mut all = Vec::with_capacity(children.len() + 1);
        all.push(constructor);
        all.extend(children);
        self.node(all)
    }
}

/// A canonical string atom that has already been admitted and scored by an
/// algebra. Keeping the atom type distinct from [`PreScoredBytes`] prevents a
/// byte-native reducer from accidentally using the byte-array score prefix for
/// a protobuf string (or vice versa).
pub struct PreScoredString<S>(S);

impl<S> PreScoredString<S> {
    pub const fn new(score: S) -> Self { Self(score) }

    pub fn into_score(self) -> S { self.0 }
}

/// A canonical byte-array atom that has already been admitted and scored by
/// an algebra.
pub struct PreScoredBytes<S>(S);

impl<S> PreScoredBytes<S> {
    pub const fn new(score: S) -> Self { Self(score) }

    pub fn into_score(self) -> S { self.0 }
}

pub fn flag(value: bool) -> i64 { i64::from(value) }

pub fn nonsemantic_score<A: CanonicalScoreAlgebra>(algebra: &mut A) -> Result<A::Score, A::Error> {
    algebra.constructor(NONSEMANTIC_SCORE_CONSTRUCTOR, Vec::new())
}

pub enum VarScore {
    Absent,
    Bound(i32),
    Free(i32),
    Wildcard,
}

pub fn var_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    score: VarScore,
) -> Result<A::Score, A::Error> {
    match score {
        VarScore::Absent => algebra.integer(i64::from(Score::ABSENT)),
        VarScore::Bound(level) => {
            let level = algebra.integer(i64::from(level))?;
            algebra.constructor(Score::BOUND_VAR, vec![level])
        }
        VarScore::Free(level) => {
            let level = algebra.integer(i64::from(level))?;
            algebra.constructor(Score::FREE_VAR, vec![level])
        }
        VarScore::Wildcard => algebra.constructor(Score::WILDCARD, Vec::new()),
    }
}

pub enum UnforgeableScore<'a> {
    Absent,
    Private(&'a [u8]),
    DeployerId(&'a [u8]),
    DeployId(&'a [u8]),
    SysAuthToken,
}

pub enum PreScoredUnforgeableScore<S> {
    Absent,
    Private(PreScoredBytes<S>),
    DeployerId(PreScoredBytes<S>),
    DeployId(PreScoredBytes<S>),
    SysAuthToken,
}

pub fn unforgeable_score_pre_scored<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    score: PreScoredUnforgeableScore<A::Score>,
) -> Result<A::Score, A::Error> {
    let (constructor, bytes) = match score {
        PreScoredUnforgeableScore::Absent => (Score::ABSENT, None),
        PreScoredUnforgeableScore::Private(bytes) => (Score::PRIVATE, Some(bytes.into_score())),
        PreScoredUnforgeableScore::DeployerId(bytes) => {
            (Score::DEPLOYER_AUTH, Some(bytes.into_score()))
        }
        PreScoredUnforgeableScore::DeployId(bytes) => (Score::DEPLOY_ID, Some(bytes.into_score())),
        PreScoredUnforgeableScore::SysAuthToken => (Score::SYS_AUTH_TOKEN, None),
    };
    algebra.constructor(constructor, bytes.into_iter().collect())
}

pub fn unforgeable_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    score: UnforgeableScore<'_>,
) -> Result<A::Score, A::Error> {
    let score = match score {
        UnforgeableScore::Absent => PreScoredUnforgeableScore::Absent,
        UnforgeableScore::Private(bytes) => {
            PreScoredUnforgeableScore::Private(PreScoredBytes::new(algebra.bytes(bytes)?))
        }
        UnforgeableScore::DeployerId(bytes) => {
            PreScoredUnforgeableScore::DeployerId(PreScoredBytes::new(algebra.bytes(bytes)?))
        }
        UnforgeableScore::DeployId(bytes) => {
            PreScoredUnforgeableScore::DeployId(PreScoredBytes::new(algebra.bytes(bytes)?))
        }
        UnforgeableScore::SysAuthToken => PreScoredUnforgeableScore::SysAuthToken,
    };
    unforgeable_score_pre_scored(algebra, score)
}

pub enum GroundScore<'a> {
    AbsentExpr,
    Bool(bool),
    Int(i64),
    String(&'a str),
    Uri(&'a str),
    ByteArray(&'a [u8]),
    Double(u64),
    BigInt(&'a [u8]),
    BigRat {
        numerator: &'a [u8],
        denominator: &'a [u8],
    },
    FixedPoint {
        unscaled: &'a [u8],
        scale: u32,
    },
}

pub enum PreScoredGroundScore<S> {
    AbsentExpr,
    Bool(bool),
    Int(i64),
    String(PreScoredString<S>),
    Uri(PreScoredString<S>),
    ByteArray(PreScoredBytes<S>),
    Double(u64),
    BigInt(PreScoredBytes<S>),
    BigRat {
        numerator: PreScoredBytes<S>,
        denominator: PreScoredBytes<S>,
    },
    FixedPoint {
        unscaled: PreScoredBytes<S>,
        scale: u32,
    },
}

pub fn ground_score_pre_scored<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    score: PreScoredGroundScore<A::Score>,
) -> Result<A::Score, A::Error> {
    match score {
        PreScoredGroundScore::AbsentExpr => algebra.constructor(Score::ABSENT, Vec::new()),
        PreScoredGroundScore::Bool(value) => {
            let value = algebra.integer(if value { 0 } else { 1 })?;
            algebra.constructor(Score::BOOL, vec![value])
        }
        PreScoredGroundScore::Int(value) => {
            let value = algebra.integer(value)?;
            algebra.constructor(Score::INT, vec![value])
        }
        PreScoredGroundScore::String(value) => {
            algebra.constructor(Score::STRING, vec![value.into_score()])
        }
        PreScoredGroundScore::Uri(value) => {
            algebra.constructor(Score::URI, vec![value.into_score()])
        }
        PreScoredGroundScore::ByteArray(value) => {
            algebra.constructor(Score::EBYTEARR, vec![value.into_score()])
        }
        PreScoredGroundScore::Double(bits) => {
            let bits = algebra.integer(bits as i64)?;
            algebra.constructor(Score::DOUBLE, vec![bits])
        }
        PreScoredGroundScore::BigInt(value) => {
            algebra.constructor(Score::BIG_INT, vec![value.into_score()])
        }
        PreScoredGroundScore::BigRat {
            numerator,
            denominator,
        } => algebra.constructor(Score::BIG_RAT, vec![
            numerator.into_score(),
            denominator.into_score(),
        ]),
        PreScoredGroundScore::FixedPoint { unscaled, scale } => {
            let scale = algebra.integer(i64::from(scale))?;
            let scale = algebra.node(vec![scale])?;
            algebra.constructor(Score::FIXED_POINT, vec![unscaled.into_score(), scale])
        }
    }
}

pub fn ground_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    score: GroundScore<'_>,
) -> Result<A::Score, A::Error> {
    let score = match score {
        GroundScore::AbsentExpr => PreScoredGroundScore::AbsentExpr,
        GroundScore::Bool(value) => PreScoredGroundScore::Bool(value),
        GroundScore::Int(value) => PreScoredGroundScore::Int(value),
        GroundScore::String(value) => {
            PreScoredGroundScore::String(PreScoredString::new(algebra.string(value)?))
        }
        GroundScore::Uri(value) => {
            PreScoredGroundScore::Uri(PreScoredString::new(algebra.string(value)?))
        }
        GroundScore::ByteArray(value) => {
            PreScoredGroundScore::ByteArray(PreScoredBytes::new(algebra.bytes(value)?))
        }
        GroundScore::Double(bits) => PreScoredGroundScore::Double(bits),
        GroundScore::BigInt(value) => {
            PreScoredGroundScore::BigInt(PreScoredBytes::new(algebra.bytes(value)?))
        }
        GroundScore::BigRat {
            numerator,
            denominator,
        } => PreScoredGroundScore::BigRat {
            numerator: PreScoredBytes::new(algebra.bytes(numerator)?),
            denominator: PreScoredBytes::new(algebra.bytes(denominator)?),
        },
        GroundScore::FixedPoint { unscaled, scale } => PreScoredGroundScore::FixedPoint {
            unscaled: PreScoredBytes::new(algebra.bytes(unscaled)?),
            scale,
        },
    };
    ground_score_pre_scored(algebra, score)
}

pub const fn expr_constructor(instance: &ExprInstance) -> i32 {
    match instance {
        ExprInstance::GBool(_) => Score::BOOL,
        ExprInstance::GInt(_) => Score::INT,
        ExprInstance::GString(_) => Score::STRING,
        ExprInstance::GUri(_) => Score::URI,
        ExprInstance::GByteArray(_) => Score::EBYTEARR,
        ExprInstance::GDouble(_) => Score::DOUBLE,
        ExprInstance::GBigInt(_) => Score::BIG_INT,
        ExprInstance::GBigRat(_) => Score::BIG_RAT,
        ExprInstance::GFixedPoint(_) => Score::FIXED_POINT,
        ExprInstance::EVarBody(_) => Score::EVAR,
        ExprInstance::ENegBody(_) => Score::ENEG,
        ExprInstance::ENotBody(_) => Score::ENOT,
        ExprInstance::EMultBody(_) => Score::EMULT,
        ExprInstance::EDivBody(_) => Score::EDIV,
        ExprInstance::EModBody(_) => Score::EMOD,
        ExprInstance::EPlusBody(_) => Score::EPLUS,
        ExprInstance::EMinusBody(_) => Score::EMINUS,
        ExprInstance::ELtBody(_) => Score::ELT,
        ExprInstance::ELteBody(_) => Score::ELTE,
        ExprInstance::EGtBody(_) => Score::EGT,
        ExprInstance::EGteBody(_) => Score::EGTE,
        ExprInstance::EEqBody(_) => Score::EEQ,
        ExprInstance::ENeqBody(_) => Score::ENEQ,
        ExprInstance::EAndBody(_) => Score::EAND,
        ExprInstance::EOrBody(_) => Score::EOR,
        ExprInstance::EMatchesBody(_) => Score::EMATCHES,
        ExprInstance::EPercentPercentBody(_) => Score::EPERCENT,
        ExprInstance::EPlusPlusBody(_) => Score::EPLUSPLUS,
        ExprInstance::EMinusMinusBody(_) => Score::EMINUSMINUS,
        ExprInstance::EListBody(_) => Score::ELIST,
        ExprInstance::ETupleBody(_) => Score::ETUPLE,
        ExprInstance::ESetBody(_) => Score::ESET,
        ExprInstance::EMapBody(_) => Score::EMAP,
        ExprInstance::EMethodBody(_) => Score::EMETHOD,
        ExprInstance::EPathmapBody(_) => rpb3_pathmap_constructor(false),
        ExprInstance::EZipperBody(_) => rpb3_pathmap_constructor(true),
    }
}

pub fn cost_signature_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    atoms: Vec<A::Score>,
) -> Result<A::Score, A::Error> {
    algebra.constructor(Score::COST_SIGNATURE, atoms)
}

pub fn cost_signature_value_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    signer: &CostSignature,
) -> Result<A::Score, A::Error> {
    let mut atoms = Vec::with_capacity(signer.atoms.len());
    for atom in &signer.atoms {
        atoms.push(match atom.form.as_ref() {
            Some(Form::Ground(bytes)) => cost_ground_score(algebra, bytes)?,
            Some(Form::Quote(bytes)) => cost_quote_score(algebra, bytes)?,
            Some(Form::RuntimeName(bytes)) => cost_runtime_name_score(algebra, bytes)?,
            Some(Form::Bound(bound)) => cost_bound_score(algebra, bound.index)?,
            None => cost_form_absent_score(algebra)?,
        });
    }
    cost_signature_score(algebra, atoms)
}

pub fn cost_token_stack_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    stack: &CostTokenStack,
) -> Result<A::Score, A::Error> {
    let mut layers = Vec::with_capacity(stack.layers.len());
    for layer in &stack.layers {
        layers.push(cost_signature_value_score(algebra, layer)?);
    }
    cost_token_stack_score_parts(algebra, layers)
}

pub fn cost_token_stack_score_parts<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    layers: Vec<A::Score>,
) -> Result<A::Score, A::Error> {
    algebra.constructor(Score::COST_TOKEN_STACK, layers)
}

pub fn cost_ground_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    ground: &[u8],
) -> Result<A::Score, A::Error> {
    let bytes = algebra.bytes(ground)?;
    algebra.constructor(Score::COST_GROUND, vec![bytes])
}

pub fn cost_quote_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    quote: &[u8],
) -> Result<A::Score, A::Error> {
    let bytes = algebra.bytes(quote)?;
    algebra.constructor(Score::COST_QUOTE, vec![bytes])
}

pub fn cost_runtime_name_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    name: &[u8],
) -> Result<A::Score, A::Error> {
    let bytes = algebra.bytes(name)?;
    algebra.constructor(Score::COST_RUNTIME_NAME, vec![bytes])
}

pub fn cost_bound_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    index: i32,
) -> Result<A::Score, A::Error> {
    let index = algebra.integer(i64::from(index))?;
    algebra.constructor(Score::COST_BOUND, vec![index])
}

pub fn cost_form_absent_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
) -> Result<A::Score, A::Error> {
    algebra.constructor(Score::COST_FORM_ABSENT, Vec::new())
}

pub fn cost_region_score_parts<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    version: u32,
    coordinates: [u64; 4],
    signer: Option<A::Score>,
    ordinal: u32,
) -> Result<A::Score, A::Error> {
    let mut children = Vec::with_capacity(7);
    children.push(algebra.integer(i64::from(version))?);
    for coordinate in coordinates {
        children.push(algebra.bytes(&coordinate.to_be_bytes())?);
    }
    children.push(match signer {
        Some(score) => score,
        None => algebra.constructor(Score::COST_SIGNER_ABSENT, Vec::new())?,
    });
    children.push(algebra.integer(i64::from(ordinal))?);
    algebra.constructor(Score::COST_REGION, children)
}

pub fn cost_region_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    region: &CostRegion,
) -> Result<A::Score, A::Error> {
    let signer = region
        .signer
        .as_ref()
        .map(|signer| cost_signature_value_score(algebra, signer))
        .transpose()?;
    cost_region_score_parts(
        algebra,
        region.version,
        [
            region.start_line,
            region.start_column,
            region.end_line,
            region.end_column,
        ],
        signer,
        region.ordinal,
    )
}

pub fn send_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    persistent: bool,
    channel: A::Score,
    data: Vec<A::Score>,
    connective_used: bool,
    cost_region: Option<A::Score>,
) -> Result<A::Score, A::Error> {
    let persistent = algebra.integer(flag(persistent))?;
    let connective_used = algebra.integer(flag(connective_used))?;
    let mut children = Vec::with_capacity(data.len() + 3);
    children.push(persistent);
    children.push(channel);
    children.extend(data);
    children.push(connective_used);
    if let Some(region) = cost_region {
        children.push(region);
    }
    algebra.constructor(Score::SEND, children)
}

pub fn receive_bind_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    source: A::Score,
    patterns: Vec<A::Score>,
    remainder: A::Score,
    cost_region: Option<A::Score>,
) -> Result<A::Score, A::Error> {
    let mut children = Vec::with_capacity(patterns.len() + 2);
    children.push(source);
    children.extend(patterns);
    children.push(remainder);
    if let Some(region) = cost_region {
        children.push(region);
    }
    algebra.node(children)
}

pub fn receive_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    persistent: bool,
    peek: bool,
    binds: Vec<A::Score>,
    body: A::Score,
    bind_count: i32,
    connective_used: bool,
    condition_or_empty: A::Score,
    cost_region: Option<A::Score>,
) -> Result<A::Score, A::Error> {
    let persistent = algebra.integer(flag(persistent))?;
    let peek = algebra.integer(flag(peek))?;
    let bind_count = algebra.integer(i64::from(bind_count))?;
    let connective_used = algebra.integer(flag(connective_used))?;
    let mut children = Vec::with_capacity(binds.len() + 6);
    children.extend([persistent, peek]);
    children.extend(binds);
    children.extend([body, bind_count, connective_used, condition_or_empty]);
    if let Some(region) = cost_region {
        children.push(region);
    }
    algebra.constructor(Score::RECEIVE, children)
}

pub fn new_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    bind_count: i32,
    uri_scores_or_absent: Vec<A::Score>,
    injection_scores_or_absent: Vec<A::Score>,
    body: A::Score,
) -> Result<A::Score, A::Error> {
    let bind_count = algebra.integer(i64::from(bind_count))?;
    let mut children =
        Vec::with_capacity(uri_scores_or_absent.len() + injection_scores_or_absent.len() + 2);
    children.push(bind_count);
    children.extend(uri_scores_or_absent);
    children.extend(injection_scores_or_absent);
    children.push(body);
    algebra.constructor(Score::NEW, children)
}

pub fn injection_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    key: &str,
    value: A::Score,
) -> Result<A::Score, A::Error> {
    let key = PreScoredString::new(algebra.string(key)?);
    injection_score_pre_scored(algebra, key, value)
}

pub fn injection_score_pre_scored<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    key: PreScoredString<A::Score>,
    value: A::Score,
) -> Result<A::Score, A::Error> {
    algebra.node(vec![key.into_score(), value])
}

pub fn match_case_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    pattern: A::Score,
    source: A::Score,
    free_count: i32,
    guard_or_empty: A::Score,
) -> Result<A::Score, A::Error> {
    let free_count = algebra.integer(i64::from(free_count))?;
    algebra.node(vec![pattern, source, free_count, guard_or_empty])
}

pub fn match_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    target: A::Score,
    cases: Vec<A::Score>,
    connective_used: bool,
) -> Result<A::Score, A::Error> {
    let connective_used = algebra.integer(flag(connective_used))?;
    let mut children = Vec::with_capacity(cases.len() + 2);
    children.push(target);
    children.extend(cases);
    children.push(connective_used);
    algebra.constructor(Score::MATCH, children)
}

pub const fn bundle_constructor(read: bool, write: bool) -> i32 {
    match (read, write) {
        (true, true) => Score::BUNDLE_READ_WRITE,
        (true, false) => Score::BUNDLE_READ,
        (false, true) => Score::BUNDLE_WRITE,
        (false, false) => Score::BUNDLE_EQUIV,
    }
}

pub fn bundle_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    read: bool,
    write: bool,
    body: A::Score,
) -> Result<A::Score, A::Error> {
    algebra.constructor(bundle_constructor(read, write), vec![body])
}

pub fn if_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    condition: A::Score,
    if_true: A::Score,
    if_false: A::Score,
    connective_used: bool,
) -> Result<A::Score, A::Error> {
    let connective_used = algebra.integer(flag(connective_used))?;
    algebra.constructor(Score::IF, vec![
        condition,
        if_true,
        if_false,
        connective_used,
    ])
}

pub enum ConnectiveScore<S> {
    Absent,
    Not(S),
    And(Vec<S>),
    Or(Vec<S>),
    VarRef { index: i32, depth: i32 },
    Bool(bool),
    Int(bool),
    String(bool),
    Uri(bool),
    ByteArray(bool),
}

pub fn connective_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    score: ConnectiveScore<A::Score>,
) -> Result<A::Score, A::Error> {
    let (constructor, children) = match score {
        ConnectiveScore::Absent => return algebra.integer(i64::from(Score::ABSENT)),
        ConnectiveScore::Not(child) => (Score::CONNECTIVE_NOT, vec![child]),
        ConnectiveScore::And(children) => (Score::CONNECTIVE_AND, children),
        ConnectiveScore::Or(children) => (Score::CONNECTIVE_OR, children),
        ConnectiveScore::VarRef { index, depth } => {
            let index = algebra.integer(i64::from(index))?;
            let depth = algebra.integer(i64::from(depth))?;
            (Score::CONNECTIVE_VARREF, vec![index, depth])
        }
        ConnectiveScore::Bool(value) => {
            let value = algebra.integer(flag(value))?;
            (Score::CONNECTIVE_BOOL, vec![value])
        }
        ConnectiveScore::Int(value) => {
            let value = algebra.integer(flag(value))?;
            (Score::CONNECTIVE_INT, vec![value])
        }
        ConnectiveScore::String(value) => {
            let value = algebra.integer(flag(value))?;
            (Score::CONNECTIVE_STRING, vec![value])
        }
        ConnectiveScore::Uri(value) => {
            let value = algebra.integer(flag(value))?;
            (Score::CONNECTIVE_URI, vec![value])
        }
        ConnectiveScore::ByteArray(value) => {
            let value = algebra.integer(flag(value))?;
            (Score::CONNECTIVE_BYTEARRAY, vec![value])
        }
    };
    algebra.constructor(constructor, children)
}

pub fn unary_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    constructor: i32,
    child: A::Score,
) -> Result<A::Score, A::Error> {
    algebra.constructor(constructor, vec![child])
}

pub fn binary_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    constructor: i32,
    first: A::Score,
    second: A::Score,
) -> Result<A::Score, A::Error> {
    algebra.constructor(constructor, vec![first, second])
}

pub fn ordered_collection_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    constructor: i32,
    remainder: Option<A::Score>,
    elements: Vec<A::Score>,
    connective_used: bool,
) -> Result<A::Score, A::Error> {
    let connective_used = algebra.integer(flag(connective_used))?;
    let mut children = Vec::with_capacity(elements.len() + usize::from(remainder.is_some()) + 1);
    if let Some(remainder) = remainder {
        children.push(remainder);
    }
    children.extend(elements);
    children.push(connective_used);
    algebra.constructor(constructor, children)
}

pub fn method_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    method_name: &str,
    target: A::Score,
    arguments: Vec<A::Score>,
    connective_used: bool,
) -> Result<A::Score, A::Error> {
    let method_name = PreScoredString::new(algebra.string(method_name)?);
    method_score_pre_scored(algebra, method_name, target, arguments, connective_used)
}

pub fn method_score_pre_scored<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    method_name: PreScoredString<A::Score>,
    target: A::Score,
    arguments: Vec<A::Score>,
    connective_used: bool,
) -> Result<A::Score, A::Error> {
    let connective_used = algebra.integer(flag(connective_used))?;
    let mut children = Vec::with_capacity(arguments.len() + 3);
    children.extend([method_name.into_score(), target]);
    children.extend(arguments);
    children.push(connective_used);
    algebra.constructor(Score::EMETHOD, children)
}

pub const fn rpb3_pathmap_constructor(is_zipper: bool) -> i32 { Score::EPATHMAP + is_zipper as i32 }

pub fn rpb3_pathmap_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    is_zipper: bool,
    canonical_snapshot: A::Score,
    connective_used: bool,
) -> Result<A::Score, A::Error> {
    let connective_used = algebra.integer(flag(connective_used))?;
    algebra.constructor(rpb3_pathmap_constructor(is_zipper), vec![
        canonical_snapshot,
        connective_used,
    ])
}

pub fn par_score<A: CanonicalScoreAlgebra>(
    algebra: &mut A,
    family_scores: Vec<A::Score>,
    connective_used: bool,
) -> Result<A::Score, A::Error> {
    let connective_used = algebra.integer(flag(connective_used))?;
    let mut children = Vec::with_capacity(family_scores.len() + 1);
    children.extend(family_scores);
    children.push(connective_used);
    algebra.constructor(Score::PAR, children)
}

/// Equality with the canonical empty `Par` observes only the component bag and
/// root connective bit. Root `locally_free` is a cache and is deliberately not
/// an input to this semantic predicate.
pub const fn par_is_semantic_default(has_components: bool, connective_used: bool) -> bool {
    !has_components && !connective_used
}

pub enum SemanticShape {
    Always,
    Send {
        channel: bool,
    },
    Receive {
        body: bool,
        bind_sources: bool,
    },
    New {
        body: bool,
    },
    Match {
        target: bool,
        cases: bool,
    },
    Bundle {
        body: bool,
    },
    If {
        condition: bool,
        if_true: bool,
        if_false: bool,
    },
    ExprVar {
        var: bool,
    },
    ExprUnary {
        child: bool,
    },
    ExprBinary {
        first: bool,
        second: bool,
    },
    ExprMatches {
        target: bool,
        pattern: bool,
    },
    ExprMap {
        pairs: bool,
    },
    ExprMethod {
        target: bool,
    },
    ExprZipper {
        pathmap: bool,
    },
}

pub const fn semantic_shape_is_complete(shape: SemanticShape) -> bool {
    match shape {
        SemanticShape::Always => true,
        SemanticShape::Send { channel } => channel,
        SemanticShape::Receive { body, bind_sources } => body && bind_sources,
        SemanticShape::New { body } => body,
        SemanticShape::Match { target, cases } => target && cases,
        SemanticShape::Bundle { body } => body,
        SemanticShape::If {
            condition,
            if_true,
            if_false,
        } => condition && if_true && if_false,
        SemanticShape::ExprVar { var } => var,
        SemanticShape::ExprUnary { child } => child,
        SemanticShape::ExprBinary { first, second } => first && second,
        SemanticShape::ExprMatches { target, pattern } => target && pattern,
        SemanticShape::ExprMap { pairs } => pairs,
        SemanticShape::ExprMethod { target } => target,
        SemanticShape::ExprZipper { pathmap } => pathmap,
    }
}

pub fn send_semantic_shape_is_complete(value: &Send) -> bool {
    semantic_shape_is_complete(SemanticShape::Send {
        channel: value.chan.is_some(),
    })
}

pub fn receive_semantic_shape_is_complete(value: &Receive) -> bool {
    semantic_shape_is_complete(SemanticShape::Receive {
        body: value.body.is_some(),
        bind_sources: value.binds.iter().all(|bind| bind.source.is_some()),
    })
}

pub fn new_semantic_shape_is_complete(value: &New) -> bool {
    semantic_shape_is_complete(SemanticShape::New {
        body: value.p.is_some(),
    })
}

pub fn match_semantic_shape_is_complete(value: &Match) -> bool {
    semantic_shape_is_complete(SemanticShape::Match {
        target: value.target.is_some(),
        cases: value
            .cases
            .iter()
            .all(|case| case.pattern.is_some() && case.source.is_some()),
    })
}

pub fn bundle_semantic_shape_is_complete(value: &Bundle) -> bool {
    semantic_shape_is_complete(SemanticShape::Bundle {
        body: value.body.is_some(),
    })
}

pub fn if_semantic_shape_is_complete(value: &If) -> bool {
    semantic_shape_is_complete(SemanticShape::If {
        condition: value.condition.is_some(),
        if_true: value.if_true.is_some(),
        if_false: value.if_false.is_some(),
    })
}

pub fn connective_semantic_shape_is_complete(_value: &Connective) -> bool { true }

pub fn expr_semantic_shape_is_complete(value: &Expr) -> bool {
    match value.expr_instance.as_ref() {
        None
        | Some(ExprInstance::GBool(_))
        | Some(ExprInstance::GInt(_))
        | Some(ExprInstance::GString(_))
        | Some(ExprInstance::GUri(_))
        | Some(ExprInstance::GByteArray(_))
        | Some(ExprInstance::GDouble(_))
        | Some(ExprInstance::GBigInt(_))
        | Some(ExprInstance::GBigRat(_))
        | Some(ExprInstance::GFixedPoint(_))
        | Some(ExprInstance::EListBody(_))
        | Some(ExprInstance::ETupleBody(_))
        | Some(ExprInstance::ESetBody(_))
        | Some(ExprInstance::EPathmapBody(_)) => semantic_shape_is_complete(SemanticShape::Always),
        Some(ExprInstance::EVarBody(value)) => semantic_shape_is_complete(SemanticShape::ExprVar {
            var: value.v.is_some(),
        }),
        Some(ExprInstance::ENegBody(value)) => expr_unary_complete(value.p.is_some()),
        Some(ExprInstance::ENotBody(value)) => expr_unary_complete(value.p.is_some()),
        Some(ExprInstance::EMultBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EDivBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EModBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EPlusBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EMinusBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::ELtBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::ELteBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EGtBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EGteBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EEqBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::ENeqBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EAndBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EOrBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EMatchesBody(value)) => {
            semantic_shape_is_complete(SemanticShape::ExprMatches {
                target: value.target.is_some(),
                pattern: value.pattern.is_some(),
            })
        }
        Some(ExprInstance::EMapBody(value)) => semantic_shape_is_complete(SemanticShape::ExprMap {
            pairs: value
                .kvs
                .iter()
                .all(|pair| pair.key.is_some() && pair.value.is_some()),
        }),
        Some(ExprInstance::EPercentPercentBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EPlusPlusBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EMinusMinusBody(value)) => {
            expr_binary_complete(value.p1.is_some(), value.p2.is_some())
        }
        Some(ExprInstance::EMethodBody(value)) => {
            semantic_shape_is_complete(SemanticShape::ExprMethod {
                target: value.target.is_some(),
            })
        }
        Some(ExprInstance::EZipperBody(value)) => {
            semantic_shape_is_complete(SemanticShape::ExprZipper {
                pathmap: value.pathmap.is_some(),
            })
        }
    }
}

const fn expr_unary_complete(child: bool) -> bool {
    semantic_shape_is_complete(SemanticShape::ExprUnary { child })
}

const fn expr_binary_complete(first: bool, second: bool) -> bool {
    semantic_shape_is_complete(SemanticShape::ExprBinary { first, second })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_tags_and_score_order_are_exhaustive_and_distinct() {
        let wire = [1, 2, 4, 5, 6, 7, 8, 11, 12, 13];
        let tags = [
            ParComponentTag::Send,
            ParComponentTag::Receive,
            ParComponentTag::New,
            ParComponentTag::Expr,
            ParComponentTag::Match,
            ParComponentTag::GUnforgeable,
            ParComponentTag::Connective,
            ParComponentTag::Bundle,
            ParComponentTag::If,
            ParComponentTag::CostTokenStack,
        ];
        assert_eq!(tags.map(ParComponentTag::byte), wire);
        assert_eq!(PAR_SCORE_TAGS.len(), tags.len());
        for tag in tags {
            assert_eq!(ParComponentTag::from_byte(tag.byte()), Some(tag));
            assert_eq!(tags[tag.index()], tag);
            assert_eq!(
                PAR_SCORE_TAGS
                    .iter()
                    .filter(|&&candidate| candidate == tag)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn rpb3_pathmap_exception_is_explicit() {
        assert_eq!(rpb3_pathmap_constructor(false), Score::EPATHMAP);
        assert_eq!(rpb3_pathmap_constructor(true), Score::DOUBLE);
    }

    #[test]
    fn every_required_semantic_shape_rejects_a_missing_slot() {
        let incomplete = [
            SemanticShape::Send { channel: false },
            SemanticShape::Receive {
                body: false,
                bind_sources: true,
            },
            SemanticShape::New { body: false },
            SemanticShape::Match {
                target: true,
                cases: false,
            },
            SemanticShape::Bundle { body: false },
            SemanticShape::If {
                condition: true,
                if_true: false,
                if_false: true,
            },
            SemanticShape::ExprVar { var: false },
            SemanticShape::ExprUnary { child: false },
            SemanticShape::ExprBinary {
                first: true,
                second: false,
            },
            SemanticShape::ExprMatches {
                target: false,
                pattern: true,
            },
            SemanticShape::ExprMap { pairs: false },
            SemanticShape::ExprMethod { target: false },
            SemanticShape::ExprZipper { pathmap: false },
        ];
        assert!(semantic_shape_is_complete(SemanticShape::Always));
        assert!(incomplete
            .into_iter()
            .all(|shape| !semantic_shape_is_complete(shape)));
    }
}
