use super::*;

pub(super) fn run(
    command: &str,
    arguments: &Arguments,
    result: &mut CommandResult,
) -> Result<(), String> {
    let input = path(&arguments.paths[0])?;
    let output = path(
        arguments
            .values
            .get("--out")
            .ok_or("Text export requires --out.")?,
    )?;
    if input == output {
        return Err("Export destination must differ from the AML source.".into());
    }
    let source = read_source(&input).map_err(|e| e.to_string())?;
    let mut scene = superspec_engine::Session::default()
        .compile_cancellable(
            &source,
            0,
            &|name| assets::read(input.parent().unwrap(), name),
            command == "obj",
            &cancelled,
        )
        .map_err(|e| e.to_string())?;
    superspec_engine::validation::audit(&mut scene, &cancelled)?;
    if !scene.manifest.success {
        return Err("Declared validation requirements failed.".into());
    }
    let options = superspec_export::Options {
        cancelled: &cancelled,
        ..Default::default()
    };
    let files = if command == "plan" {
        vec![(output, superspec_export::svg_plan(&scene, &options)?)]
    } else {
        let object = superspec_export::obj(&scene, &options)?;
        let mut files = vec![
            (output.join("scene.obj"), object.geometry),
            (output.join("scene.mtl"), object.materials),
        ];
        files.extend(
            object
                .textures
                .into_iter()
                .map(|(name, bytes)| (output.join(name), bytes)),
        );
        files
    };
    result.source = Some(input);
    result.artifacts = publish_files(files)?;
    result.diagnostics = scene.manifest.diagnostics;
    Ok(())
}
