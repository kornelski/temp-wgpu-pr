use crate::arena::Arena;
use std::{num::NonZeroU32, ops};

pub type Alignment = NonZeroU32;

/// Alignment information for a type.
#[derive(Clone, Copy, Debug, Hash, PartialEq)]
pub struct TypeLayout {
    pub size: u32,
    pub alignment: Alignment,
}

/// Helper processor that derives the sizes of all types.
/// It uses the default layout algorithm/table, described in
/// https://github.com/gpuweb/gpuweb/issues/1393
#[derive(Debug, Default)]
pub struct Layouter {
    layouts: Vec<TypeLayout>,
}

impl Layouter {
    pub fn new(types: &Arena<crate::Type>, constants: &Arena<crate::Constant>) -> Self {
        let mut this = Layouter::default();
        this.initialize(types, constants);
        this
    }

    pub fn round_up(alignment: NonZeroU32, offset: u32) -> u32 {
        match offset & alignment.get() {
            0 => offset,
            other => offset + alignment.get() - other,
        }
    }

    pub fn member_placement(
        &self,
        offset: u32,
        member: &crate::StructMember,
    ) -> (ops::Range<u32>, NonZeroU32) {
        let layout = self.layouts[member.ty.index()];
        let alignment = member.align.unwrap_or(layout.alignment);
        let start = Self::round_up(alignment, offset);
        let end = start
            + match member.size {
                Some(size) => size.get(),
                None => layout.size,
            };
        (start..end, alignment)
    }

    pub fn initialize(&mut self, types: &Arena<crate::Type>, constants: &Arena<crate::Constant>) {
        use crate::TypeInner as Ti;

        self.layouts.clear();
        self.layouts.reserve(types.len());

        for (_, ty) in types.iter() {
            self.layouts.push(match ty.inner {
                Ti::Scalar { kind: _, width } => TypeLayout {
                    size: width as u32,
                    alignment: Alignment::new(width as u32).unwrap(),
                },
                Ti::Vector {
                    size,
                    kind: _,
                    width,
                } => TypeLayout {
                    size: (size as u8 * width) as u32,
                    alignment: {
                        let count = if size >= crate::VectorSize::Tri { 4 } else { 2 };
                        Alignment::new((count * width) as u32).unwrap()
                    },
                },
                Ti::Matrix {
                    columns,
                    rows,
                    width,
                } => TypeLayout {
                    size: (columns as u8 * rows as u8 * width) as u32,
                    alignment: {
                        let count = if rows >= crate::VectorSize::Tri { 4 } else { 2 };
                        Alignment::new((count * width) as u32).unwrap()
                    },
                },
                Ti::Pointer { .. } | Ti::ValuePointer { .. } => TypeLayout {
                    size: 4,
                    alignment: Alignment::new(1).unwrap(),
                },
                Ti::Array { base, size, stride } => {
                    let count = match size {
                        crate::ArraySize::Constant(handle) => match constants[handle].inner {
                            crate::ConstantInner::Scalar {
                                width: _,
                                value: crate::ScalarValue::Uint(value),
                            } => value as u32,
                            // Accept a signed integer size to avoid
                            // requiring an explicit uint
                            // literal. Type inference should make
                            // this unnecessary.
                            crate::ConstantInner::Scalar {
                                width: _,
                                value: crate::ScalarValue::Sint(value),
                            } => value as u32,
                            ref other => unreachable!("Unexpected array size {:?}", other),
                        },
                        crate::ArraySize::Dynamic => 0,
                    };
                    let stride = match stride {
                        Some(value) => value,
                        None => {
                            let layout = &self.layouts[base.index()];
                            let stride = Self::round_up(layout.alignment, layout.size);
                            Alignment::new(stride).unwrap()
                        }
                    };
                    TypeLayout {
                        size: count * stride.get(),
                        alignment: stride,
                    }
                }
                Ti::Struct {
                    block: _,
                    ref members,
                } => {
                    let mut total = 0;
                    let mut biggest_alignment = Alignment::new(1).unwrap();
                    for member in members {
                        let (placement, alignment) = self.member_placement(total, member);
                        biggest_alignment = biggest_alignment.max(alignment);
                        total = placement.end;
                    }
                    TypeLayout {
                        size: Self::round_up(biggest_alignment, total),
                        alignment: biggest_alignment,
                    }
                }
                Ti::Image { .. } | Ti::Sampler { .. } => TypeLayout {
                    size: 0,
                    alignment: Alignment::new(1).unwrap(),
                },
            });
        }
    }

    pub fn resolve(&self, handle: crate::Handle<crate::Type>) -> TypeLayout {
        self.layouts[handle.index()]
    }
}
