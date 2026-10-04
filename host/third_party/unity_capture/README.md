# Unity Capture shared memory

`shared.inl` is adapted from `schellingb/UnityCapture`,
commit `3ed54c325e0ad71afcf4f246c07e5e17b3d7f2d2`, `Source/shared.inl`.
The filter's MIT notice is preserved in `LICENSE`.

Local changes remove unused global compiler macros/GUID definitions, unmap the
view on destruction, request the mutex/event rights required for writing and
waiting, use a numeric zero for the mapping size, and bound producer mutex waits.
The shared layout and names remain compatible with the pinned filter DLLs.
The Unity plugin is not used or copied.
