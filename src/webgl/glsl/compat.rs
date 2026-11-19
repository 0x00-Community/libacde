use naga::proc::{ConstValueError, GlobalCtx};
use naga::{Arena, Binding, Expression, Handle, Interpolation, Literal, Sampling, Span, TypeInner};

pub fn span(location: super::preprocessor::token::Location) -> Span {
    Span::new(location.start, location.end)
}

pub fn apply_default_interpolation(binding: &mut Binding, ty: &TypeInner) {
    let Binding::Location { interpolation: ref mut interpolation @ None, ref mut sampling, .. } = *binding else {
        return;
    };
    if let Some(naga::ScalarKind::Float) = ty.scalar_kind() {
        *interpolation = Some(Interpolation::Perspective);
        *sampling = Some(Sampling::Center);
    }
}

pub trait ConstValue {
    fn get_const_val<T, E>(&self, handle: Handle<Expression>) -> Result<T, ConstValueError>
    where
        T: TryFrom<Literal, Error = E>,
        E: Into<ConstValueError>;

    fn get_const_val_from<T, E>(&self, handle: Handle<Expression>, arena: &Arena<Expression>) -> Result<T, ConstValueError>
    where
        T: TryFrom<Literal, Error = E>,
        E: Into<ConstValueError>;
}

impl ConstValue for GlobalCtx<'_> {
    fn get_const_val<T, E>(&self, handle: Handle<Expression>) -> Result<T, ConstValueError>
    where
        T: TryFrom<Literal, Error = E>,
        E: Into<ConstValueError>,
    {
        self.get_const_val_from(handle, self.global_expressions)
    }

    fn get_const_val_from<T, E>(&self, handle: Handle<Expression>, arena: &Arena<Expression>) -> Result<T, ConstValueError>
    where
        T: TryFrom<Literal, Error = E>,
        E: Into<ConstValueError>,
    {
        let literal = |handle: Handle<Expression>, arena: &Arena<Expression>| match arena[handle] {
            Expression::Literal(literal) => Some(literal),
            Expression::ZeroValue(ty) => match self.types[ty].inner {
                TypeInner::Scalar(scalar) => Literal::zero(scalar),
                _ => None,
            },
            _ => None,
        };
        let value = match arena[handle] {
            Expression::Constant(c) => literal(self.constants[c].init, self.global_expressions),
            _ => literal(handle, arena),
        };
        match value {
            Some(v) => v.try_into().map_err(Into::into),
            None => Err(ConstValueError::NonConst),
        }
    }
}
