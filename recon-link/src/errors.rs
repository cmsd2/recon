use recon_util;

error_chain! {
    errors {
        ReceiveError
        SendError
        TransportError(s: &'static str) {
            description("transport error")
            display("transport error: {}", s)
        }
    }

    links {
        ReconUtilError(recon_util::errors::Error, recon_util::errors::ErrorKind);
    }
}