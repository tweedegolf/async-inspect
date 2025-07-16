fn main() {
    let a = std::hint::black_box(4);

    let bar = async move {
        let b = foo_1().await;
        let c = foo_2().await;
        let mid = a + b + c;
        let d = foo_3::<i32>().await;
        let e = foo_1().await;
        mid + d + e
    };

    let value = futures::executor::block_on(bar);
    println!("{}", value);
}

// async fn bar() -> i32 {
// }

async fn foo_1() -> i32 {
    let a = std::hint::black_box(5);
    let b = foo_2().await;
    let c = foo_3::<i32>().await;
    a + b + c
}

async fn foo_2() -> i32 {
    std::hint::black_box(2)
}

async fn foo_3<T: Default>() -> T {
    std::hint::black_box(T::default())
}
