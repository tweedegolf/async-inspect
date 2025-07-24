fn main() {
    async_inspect::initialize();

    let mut a = std::hint::black_box(4);

    // let bar = async move {
    //     let b = foo_1().await;
    //     let c = foo_2().await;
    //     a += b;
    //     let d = foo_3::<i32>().await;
    //     a += c;
    //     let e = foo_2().await;
    //     a += d;
    //     let f = foo_3::<i32>().await;
    //     a += e;
    //     let g = foo_1().await;
    //     a += f;
    //     let h = foo_2().await;
    //     a += g;
    //     a += h;
    //     a
    // };

    let bar = async move {
        let b = foo_1().await;
        let c = foo_2().await;
        let d = foo_3::<i32>().await;
        let mid = b + c + d;
        let e = foo_2().await;
        let f = foo_3::<i32>().await;
        let g = foo_1().await;
        let h = foo_2().await;
        a + mid + e + f + g + h
    };

    // let bar = async move {
    //     let b = foo_3::<i32>().await;
    //     let c = foo_3::<i32>().await;
    //     let mid = b + c;
    //     let d = foo_3::<i32>().await;
    //     let e = foo_3::<i32>().await;
    //     mid + d + e
    // };

    let value = futures::executor::block_on(async_inspect::inspect(bar));
    println!("{}", value);
}

async fn foo_1() -> i32 {
    let test = std::hint::black_box("Test".to_owned());
    let a = std::hint::black_box(5);
    let b = foo_2().await;
    let c = foo_3::<i32>().await;
    a + b + c
}

fn foo_2() -> impl Future<Output = i32> {
    let mut polled = 0;
    let f = std::future::poll_fn(move |cx| {
        if polled >= 4 {
            std::task::Poll::Ready(7)
        } else {
            polled += 1;

            cx.waker().clone().wake();

            std::task::Poll::Pending
        }
    });
    f
}

async fn foo_3<T: Default>() -> T {
    std::hint::black_box(T::default())
}
