use wasm_bindgen::prelude::*;

fn replace(shader: &mut String, marker: &str, source: &str) -> Result<(), JsValue> {
    if !shader.contains(marker) {
        return Err(JsValue::from_str(&format!(
            "Missing Three.js shader marker {marker}."
        )));
    }
    *shader = shader.replacen(marker, source, 1);
    Ok(())
}

/// Assembles the concrete study's geometry attributes and material shaders.
#[wasm_bindgen]
pub fn concrete_study_shader(vertex: bool, mut source: String) -> Result<String, JsValue> {
    if vertex {
        let mut declarations = String::from(
            "#include <common>\nattribute vec2 concreteFaceHalfSize;\nattribute float concreteTopDistance;\nattribute vec2 concreteGroundContact;\nvarying float vConcreteTopDistance;\nvarying vec2 vConcreteFaceHalfSize;\nvarying vec2 vConcreteGroundContact;\nvarying vec3 vConcreteLocalPosition;\nvarying vec4 vConcreteEdges[4];\n",
        );
        let mut attributes = String::from(
            "#include <begin_vertex>\nvConcreteFaceHalfSize = concreteFaceHalfSize;\nvConcreteTopDistance = concreteTopDistance;\nvConcreteGroundContact = concreteGroundContact;\nvConcreteLocalPosition = position;\n",
        );
        for slot in 0..4 {
            use std::fmt::Write;
            writeln!(declarations, "attribute vec4 concreteEdge{slot};").unwrap();
            writeln!(attributes, "vConcreteEdges[{slot}] = vec4(normalMatrix * concreteEdge{slot}.xyz, dot(concreteEdge{slot}.xyz, position) + concreteEdge{slot}.w);").unwrap();
        }
        replace(&mut source, "#include <common>", &declarations)?;
        replace(&mut source, "#include <begin_vertex>", &attributes)?;
    } else {
        replace(
            &mut source,
            "#include <map_pars_fragment>",
            concat!(
                "#include <map_pars_fragment>\nuniform float concreteChipFinishEnabled;\n",
                include_str!("study_shaders/concrete_sampling.glsl")
            ),
        )?;
        for (marker, body) in [
            (
                "#include <map_fragment>",
                include_str!("study_shaders/concrete_map.glsl"),
            ),
            (
                "#include <roughnessmap_fragment>",
                include_str!("study_shaders/concrete_roughness.glsl"),
            ),
            (
                "#include <normal_fragment_maps>",
                include_str!("study_shaders/concrete_normal.glsl"),
            ),
        ] {
            replace(&mut source, marker, body)?;
        }
    }
    Ok(source)
}

/// Assembles the metal study's oxide, polished edge, and reflection shaders.
#[wasm_bindgen]
pub fn metal_study_shader(
    vertex: bool,
    mut source: String,
    mut environment: String,
) -> Result<String, JsValue> {
    if vertex {
        replace(
            &mut source,
            "#include <common>",
            include_str!("study_shaders/metal_vertex.glsl"),
        )?;
        replace(
            &mut source,
            "#include <project_vertex>",
            include_str!("study_shaders/metal_projection.glsl"),
        )?;
    } else {
        replace(
            &mut environment,
            "vec4 envMapColor = textureCubeUV( envMap, envMapRotation * reflectVec, roughness );",
            include_str!("study_shaders/metal_reflection.glsl"),
        )?;
        replace(
            &mut source,
            "#include <envmap_physical_pars_fragment>",
            &environment,
        )?;
        for (marker, body) in [
            (
                "#include <common>",
                include_str!("study_shaders/metal_fragment.glsl"),
            ),
            (
                "#include <metalnessmap_fragment>",
                include_str!("study_shaders/metal_oxide.glsl"),
            ),
            (
                "#include <normal_fragment_maps>",
                include_str!("study_shaders/metal_normal.glsl"),
            ),
            (
                "#include <lights_physical_fragment>",
                include_str!("study_shaders/metal_lighting.glsl"),
            ),
            (
                "#include <opaque_fragment>",
                include_str!("study_shaders/metal_output.glsl"),
            ),
        ] {
            replace(&mut source, marker, body)?;
        }
    }
    Ok(source)
}
