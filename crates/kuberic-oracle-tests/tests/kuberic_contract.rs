#![cfg(feature = "kuberic")]

use kuberic_oracle::kuberic::{
    KubericAdapterError, ObservationFailureKind, ObservationSourceError, OracleServiceConfig,
    UnsupportedOperation,
};

#[test]
fn public_adapter_types_expose_typed_errors() {
    let _config_type = std::any::type_name::<OracleServiceConfig>();
    let source_error = ObservationSourceError::new(ObservationFailureKind::Deadline);
    assert_eq!(
        source_error.to_string(),
        "Oracle observation failed: deadline exceeded"
    );
    assert!(
        KubericAdapterError::UnsupportedOperation(UnsupportedOperation::BuildReplica)
            .to_string()
            .contains("replica build")
    );
}
