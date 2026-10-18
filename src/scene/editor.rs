use crate::desktop::editor_commands::DesktopEditorCommandsContext;
use crate::scene::renderer::SceneRendererGeneration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneEditorBinding {
    context: DesktopEditorCommandsContext,
    generation: SceneRendererGeneration,
}

impl SceneEditorBinding {
    pub const fn new(
        context: DesktopEditorCommandsContext,
        generation: SceneRendererGeneration,
    ) -> Self {
        Self {
            context,
            generation,
        }
    }

    pub const fn context(self) -> DesktopEditorCommandsContext {
        self.context
    }

    pub const fn generation(self) -> SceneRendererGeneration {
        self.generation
    }
}
