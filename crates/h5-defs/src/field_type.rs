//! The field type table (PRD §6.4.4): every `_field_*` element the
//! definitions use, with its size in the tag data. One row per type, here and
//! nowhere else. Sources: AusarDocs `CachedTag.md` "Tag Data" for tag blocks,
//! data references, tag references, pageable resources and string_ids; the
//! rest by observation: definition sizes compared with real data block sizes
//! (docs/format-notes.md "Field types").

/// How many bytes a field takes in its struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Fixed(u32),
    /// The sum of its children (structs, arrays: one instance, PRD §12 Q7).
    Children,
    /// The field's `length` attribute (padding, skipped bytes).
    Length,
}

macro_rules! field_types {
    ($($variant:ident $xml:literal $size:expr;)*) => {
        /// A definition field's type: the XML element name.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum FieldType {
            $($variant,)*
        }

        impl FieldType {
            pub const ALL: &[FieldType] = &[$(FieldType::$variant,)*];

            /// The XML element name (`_field_real`).
            pub fn xml_name(self) -> &'static str {
                match self { $(FieldType::$variant => $xml,)* }
            }

            pub fn from_xml(name: &str) -> Option<FieldType> {
                match name { $($xml => Some(FieldType::$variant),)* _ => None }
            }

            pub fn size(self) -> Size {
                use Size::*;
                match self { $(FieldType::$variant => $size,)* }
            }
        }
    };
}

field_types! {
    // Containers and zero-size fields.
    Struct "_field_struct" Children;
    Array "_field_array" Children;
    Explanation "_field_explanation" Fixed(0);
    Custom "_field_custom" Fixed(0);
    Pad "_field_pad" Length;
    Skip "_field_skip" Length;
    // AusarDocs sizes.
    Block64 "_field_block_64" Fixed(0x1C);
    Data64 "_field_data_64" Fixed(0x1C);
    TagReference64 "_field_tag_reference_64" Fixed(0x20);
    PageableResource64 "_field_pageable_resource_64" Fixed(0x10);
    StringId "_field_string_id" Fixed(4);
    // Integers.
    CharInteger "_field_char_integer" Fixed(1);
    ByteInteger "_field_byte_integer" Fixed(1);
    ShortInteger "_field_short_integer" Fixed(2);
    WordInteger "_field_word_integer" Fixed(2);
    LongInteger "_field_long_integer" Fixed(4);
    DwordInteger "_field_dword_integer" Fixed(4);
    Int64Integer "_field_int64_integer" Fixed(8);
    QwordInteger "_field_qword_integer" Fixed(8);
    ShortIntegerBounds "_field_short_integer_bounds" Fixed(4);
    // Enums and flags.
    CharEnum "_field_char_enum" Fixed(1);
    ShortEnum "_field_short_enum" Fixed(2);
    LongEnum "_field_long_enum" Fixed(4);
    ByteFlags "_field_byte_flags" Fixed(1);
    WordFlags "_field_word_flags" Fixed(2);
    LongFlags "_field_long_flags" Fixed(4);
    ByteBlockFlags "_field_byte_block_flags" Fixed(1);
    WordBlockFlags "_field_word_block_flags" Fixed(2);
    LongBlockFlags "_field_long_block_flags" Fixed(4);
    // Block indices.
    CharBlockIndex "_field_char_block_index" Fixed(1);
    ShortBlockIndex "_field_short_block_index" Fixed(2);
    LongBlockIndex "_field_long_block_index" Fixed(4);
    CustomShortBlockIndex "_field_custom_short_block_index" Fixed(2);
    CustomLongBlockIndex "_field_custom_long_block_index" Fixed(4);
    // Reals and multi-component values.
    Real "_field_real" Fixed(4);
    RealFraction "_field_real_fraction" Fixed(4);
    Angle "_field_angle" Fixed(4);
    RealBounds "_field_real_bounds" Fixed(8);
    AngleBounds "_field_angle_bounds" Fixed(8);
    FractionBounds "_field_fraction_bounds" Fixed(8);
    RealPoint2d "_field_real_point_2d" Fixed(8);
    RealPoint3d "_field_real_point_3d" Fixed(12);
    RealVector2d "_field_real_vector_2d" Fixed(8);
    RealVector3d "_field_real_vector_3d" Fixed(12);
    RealEulerAngles2d "_field_real_euler_angles_2d" Fixed(8);
    RealEulerAngles3d "_field_real_euler_angles_3d" Fixed(12);
    RealQuaternion "_field_real_quaternion" Fixed(16);
    RealPlane2d "_field_real_plane_2d" Fixed(12);
    RealPlane3d "_field_real_plane_3d" Fixed(16);
    Point2d "_field_point_2d" Fixed(4);
    Rectangle2d "_field_rectangle_2d" Fixed(8);
    // Colours.
    RgbColor "_field_rgb_color" Fixed(4);
    ArgbColor "_field_argb_color" Fixed(4);
    RealRgbColor "_field_real_rgb_color" Fixed(12);
    RealArgbColor "_field_real_argb_color" Fixed(16);
    RealHsvColor "_field_real_hsv_color" Fixed(12);
    RealAhsvColor "_field_real_ahsv_color" Fixed(16);
    // Text and names.
    String "_field_string" Fixed(32);
    LongString "_field_long_string" Fixed(256);
    OldStringId "_field_old_string_id" Fixed(4);
    Tag "_field_tag" Fixed(4);
}
