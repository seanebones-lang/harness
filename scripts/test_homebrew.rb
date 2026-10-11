# Actual Homebrew DSL with disposable payloads; no network or user-prefix install.
require "formulary"
require "tmpdir"
require "fileutils"
Dir.mktmpdir("harness-formula-contract-") do |root|
  path = Pathname(root)/"harness.rb"
  path.write(File.read(ARGV.fetch(0)).gsub("REPLACE_WITH_ACTUAL_SHA", "0"*64))
  klass = Formulary.load_formula_from_path("harness", path, flags: [], ignore_errors: false)
  formula = klass.new("harness", path, :stable)
  raise "Wrong native source" unless %w[harness-macos-aarch64 harness-macos-x86_64 harness-linux-aarch64 harness-linux-x86_64].include?(File.basename(formula.stable.url))
  formula.resources.each do |resource|
    raise "License version mismatch" unless resource.url.include?("/v#{formula.version}/")
  end
  install_bin = Pathname(root)/"installed/bin"
  install_share = Pathname(root)/"installed/share"
  formula.define_singleton_method(:bin) { install_bin }
  formula.define_singleton_method(:pkgshare) { install_share }
  work = Pathname(root)/"work"; work.mkpath
  (work/File.basename(formula.stable.url)).write("synthetic executable")
  names = {"harness-license"=>"LICENSE.txt", "dependency-licenses"=>"THIRD-PARTY-LICENSES.html"}
  formula.resources.each do |resource|
    stage = Pathname(root)/resource.name;stage.mkpath
    (stage/names.fetch(resource.name)).write("Synthetic license fixture #{resource.name}")
    resource.define_singleton_method(:stage) { |&block| Dir.chdir(stage) { block.call } }
  end
  Dir.chdir(work) { formula.install }
  raise "Binary not renamed" unless (install_bin/"harness").read=="synthetic executable"
  names.values.each { |name| raise "Notice missing" unless (install_share/name).file? }
  puts "PASS actual Homebrew formula parsing, versioned resources, binary renaming and license staging in disposable folders"
end
