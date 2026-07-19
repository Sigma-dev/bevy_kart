//! Small helpers for authoring spawns with the `bsn!` macro.

use bevy::ecs::error::Result;
use bevy::ecs::template::{Template, TemplateContext};
use bevy::prelude::*;
use bevy::scene::{ResolveContext, ResolveSceneError, ResolvedScene};

/// A [`Template`] that yields a clone of an owned [`Bundle`].
struct BundleTemplate<B: Bundle + Clone>(B);

impl<B: Bundle + Clone + Send + Sync + 'static> Template for BundleTemplate<B> {
    type Output = B;

    fn build_template(&self, _context: &mut TemplateContext) -> Result<B> {
        Ok(self.0.clone())
    }

    fn clone_template(&self) -> Self {
        BundleTemplate(self.0.clone())
    }
}

/// A [`Scene`] that inserts an owned [`Bundle`] as-is.
struct InsertBundle<B: Bundle + Clone>(B);

impl<B: Bundle + Clone + Send + Sync + 'static> Scene for InsertBundle<B> {
    fn resolve(
        self,
        _context: &mut ResolveContext,
        scene: &mut ResolvedScene,
    ) -> core::result::Result<(), ResolveSceneError> {
        scene.push_bundle_template(BundleTemplate(self.0));
        Ok(())
    }
}

/// Insert an already-constructed [`Bundle`] inside a `bsn!` scene, e.g.
/// `bsn! { {insert(Sprite::from_image(handle))} SomeMarker }`.
///
/// `bsn!`'s template system is built around asset *paths*; this is the clean
/// escape hatch for components built from preloaded handles (atlas `Sprite`s /
/// `ImageNode`s, `Mesh2d`, `MeshMaterial2d`, ...) that don't map onto it neatly.
pub fn insert(bundle: impl Bundle + Clone + Send + Sync + 'static) -> impl Scene {
    InsertBundle(bundle)
}
