# get_unchecked
A Rust library that you will never use it.

## Usage
```rust
struct A(pub i32);

impl A
{
    pub fn add_one(&mut self)
    {
        self.0 += 1;
    }
}

#[get_unchecked(exclude = [sub2])]
fn main()
{
    let mut arr = vec![1, 2, 3, 4, 5, 6];
    let sub = &mut arr[..3];
    sub[3] += sub[4];

    assert_eq!(arr[3], 9);

    let err = panic::catch_unwind(move ||
    {
        let sub2 = &mut arr[..3];
        sub2[3] += 1;
    });

    assert!(err.is_err());

    let mut arr = vec![A(0), A(1), A(2)];

    foo(&mut arr[..1]);
    assert_eq!(arr[1].0, 2);
}

#[get_unchecked(mut = [add_one])]
fn foo(arr: &mut [A])
{
    arr[1].add_one();
}
```
P.S. The macro active only in release mode
