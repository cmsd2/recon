
error_chain! { 

    errors {
        LockError(s: String) {
            description("error acquiring lock")
            display("error acquiring lock: {}", s)
        }

        PollError
    }
}