use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(super) fn get(window: &impl HasWindowHandle) -> anyhow::Result<RawWindowHandle> {
    // GPUI has an inherent method with the same name. HandleError also lacks
    // std::error::Error when the dependency's optional std feature is disabled.
    HasWindowHandle::window_handle(window)
        .map(|handle| handle.as_raw())
        .map_err(|error| anyhow::anyhow!("native window handle unavailable: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct UnavailableWindow;

    impl HasWindowHandle for UnavailableWindow {
        fn window_handle(
            &self,
        ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
            Err(raw_window_handle::HandleError::Unavailable)
        }
    }

    #[test]
    fn unavailable_native_window_is_a_contextual_error_not_a_panic() {
        let error = get(&UnavailableWindow).unwrap_err().to_string();
        assert!(error.contains("native window handle unavailable"));
        assert!(error.contains("Unavailable"));
    }
}
