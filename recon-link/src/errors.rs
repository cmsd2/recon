use recon_util;

error_chain! {
    errors {

    }

    links {
        ReconUtilError(recon_util::errors::Error, recon_util::errors::ErrorKind);
    }
}