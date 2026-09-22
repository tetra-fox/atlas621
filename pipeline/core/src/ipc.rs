use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use arrow::array::{
    ArrayRef, Float32Array, Int8Array, StringArray, UInt8Array, UInt16Array, UInt32Array,
};
use arrow::buffer::OffsetBuffer;
use arrow::datatypes::{DataType, Field, Schema};
use arrow::ipc::writer::FileWriter;
use arrow::record_batch::RecordBatch;

pub trait Column: Copy {
    fn column(values: &[Self]) -> ArrayRef;
}

macro_rules! column {
    ($t:ty, $arr:ty) => {
        impl Column for $t {
            fn column(values: &[Self]) -> ArrayRef {
                Arc::new(<$arr>::from_iter_values(values.iter().copied()))
            }
        }
    };
}
column!(u8, UInt8Array);
column!(u16, UInt16Array);
column!(u32, UInt32Array);
column!(f32, Float32Array);
column!(i8, Int8Array);

pub fn strings(values: &[String]) -> ArrayRef {
    Arc::new(StringArray::from_iter_values(values))
}

// a ragged run per row, stored as arrow offsets over one flat values buffer, which is the same
// shape the reader wants back
pub fn runs<T: Column>(offsets: &[u32], values: &[T]) -> Result<ArrayRef> {
    let field = Arc::new(Field::new("item", element_type::<T>(), false));
    let offsets = OffsetBuffer::new(offsets.iter().map(|&o| o as i32).collect());
    Ok(Arc::new(arrow::array::ListArray::try_new(
        field,
        offsets,
        T::column(values),
        None,
    )?))
}

// a fixed width run per row, for rows that all carry the same count
pub fn rows<T: Column>(width: usize, values: &[T]) -> Result<ArrayRef> {
    let field = Arc::new(Field::new("item", element_type::<T>(), false));
    Ok(Arc::new(arrow::array::FixedSizeListArray::try_new(
        field,
        width as i32,
        T::column(values),
        None,
    )?))
}

fn element_type<T: Column>() -> DataType {
    T::column(&[]).data_type().clone()
}

pub fn write<W: Write>(w: W, columns: &[(&str, ArrayRef)]) -> Result<()> {
    let fields: Vec<Field> = columns
        .iter()
        .map(|(name, a)| Field::new(*name, a.data_type().clone(), false))
        .collect();
    let schema = Arc::new(Schema::new(fields));
    let arrays: Vec<ArrayRef> = columns.iter().map(|(_, a)| Arc::clone(a)).collect();
    let mut out = FileWriter::try_new(w, &schema)?;
    out.write(&RecordBatch::try_new(Arc::clone(&schema), arrays)?)?;
    out.finish()?;
    Ok(())
}

pub fn one<T: Column>(w: impl Write, name: &str, values: &[T]) -> Result<()> {
    write(w, &[(name, T::column(values))])
}

fn first_batch(bytes: &[u8]) -> Result<RecordBatch> {
    let mut reader = arrow::ipc::reader::FileReader::try_new(std::io::Cursor::new(bytes), None)?;
    reader
        .next()
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("arrow file has no record batch"))
}

fn column_of<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a ArrayRef> {
    batch
        .column_by_name(name)
        .ok_or_else(|| anyhow::anyhow!("arrow file has no {name} column"))
}

pub fn read_pairs(bytes: &[u8], name: &str) -> Result<Vec<[f32; 2]>> {
    let batch = first_batch(bytes)?;
    let list = column_of(&batch, name)?
        .as_any()
        .downcast_ref::<arrow::array::FixedSizeListArray>()
        .ok_or_else(|| anyhow::anyhow!("{name} is not a fixed size list"))?;
    let flat = list
        .values()
        .as_any()
        .downcast_ref::<Float32Array>()
        .ok_or_else(|| anyhow::anyhow!("{name} does not hold f32"))?;
    Ok(flat.values().as_chunks::<2>().0.to_vec())
}

pub fn read_strings(bytes: &[u8], name: &str) -> Result<Vec<String>> {
    let batch = first_batch(bytes)?;
    let text = column_of(&batch, name)?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| anyhow::anyhow!("{name} is not a string column"))?;
    Ok(text.iter().map(|s| s.unwrap_or_default().to_string()).collect())
}
