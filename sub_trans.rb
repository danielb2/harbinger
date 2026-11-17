#!/usr/bin/env ruby

require 'json'
require 'fileutils'
require 'uri'
require 'thread'
require 'net/http'
require 'timeout'

unless ARGV.first == 'test'
  require 'progress_bar'
  class Array; include ProgressBar::WithProgress; end
end

module SubtitleTranslator
  module_function

  RateLimitError = Class.new(StandardError)

  def parse_srt(block)
    lines = block.split("\n")
    { kind: :srt, original: block, index: lines[0], timestamp: lines[1], text: lines[2..].join("\n") }
  end

  def parse_ass(line)
    return nil unless line.match?(/^Dialogue\s*:/i)

    fields = line.sub(/^Dialogue\s*:\s*/i, '').split(',', 10)
    return nil if fields.length < 10

    { kind: :ass, original: line, prefix: line[0...line.index(':') + 1], fields: fields, text: fields[9] }
  end

  def translate_entries(entries, jobs, source_lang, target_lang, translator: method(:translate_text))
    jobs = [jobs.to_i, 1].max
    work = Queue.new
    entries.each_with_index { |entry, index| work << [index, entry] }
    jobs.times { work << nil }
    results = Array.new(entries.length)
    errors = Array.new(entries.length)
    completed = Queue.new

    workers = jobs.times.map do
      Thread.new do
        while (item = work.pop)
          index, entry = item
          begin
            results[index] = translator.call(entry[:text], sl: source_lang, tl: target_lang)
          rescue StandardError => error
            errors[index] = error
          ensure
            completed << index
          end
        end
      end
    end
    bar = ProgressBar.new(entries.length)
    bar.write
    next_to_write = 0
    completed_count = 0
    while completed_count < entries.length
      completed.pop
      completed_count += 1
      bar.increment!
      while next_to_write < entries.length && (results[next_to_write] || errors[next_to_write])
        raise errors[next_to_write] if errors[next_to_write]
        yield(next_to_write, results[next_to_write]) if block_given?
        next_to_write += 1
      end
    end
    workers.each(&:join)
    results
  end

  def rate_limit_message(retry_after)
    wait = retry_after && !retry_after.empty? ? " Retry-After: #{retry_after}." : " No Retry-After header was provided."
    "Google Translate rate limit (HTTP 429). Translation stopped.#{wait}"
  end

  def network_retry_delay(attempt)
    attempt * attempt
  end

  def translate_text(text, sl:, tl:)
    uri = URI('https://translate.googleapis.com/translate_a/single')
    uri.query = URI.encode_www_form(
      'client' => 'gtx', 'sl' => sl, 'tl' => tl, 'dt' => 't', 'q' => text
    )
    attempts = 0
    begin
      response = Net::HTTP.start(uri.host, uri.port, use_ssl: uri.scheme == 'https') do |http|
        http.get(uri.request_uri)
      end
    rescue IOError, SocketError, SystemCallError, Timeout::Error => error
      raise "Translation network error after #{attempts} retries: #{error.message}" if attempts >= 4

      attempts += 1
      sleep(network_retry_delay(attempts))
      retry
    end
    if response.code.to_i == 429
      raise RateLimitError, rate_limit_message(response['retry-after'])
    end
    raise "HTTP error #{response.code} #{response.message}" unless response.is_a?(Net::HTTPSuccess)

    (JSON.parse(response.body.to_s)[0] || []).map { |segment| segment[0] }.join
  end

  def source_language(file)
    match = File.basename(file).match(/\.([a-z]{2,3})(?:\.cc)?\.(?:srt|ass)$/i)
    match ? match[1].downcase : 'en'
  end

  def output_path(file, language)
    basename = File.basename(file)
    if basename.match?(/\.[a-z]{2,3}(?:\.cc)?\.(?:srt|ass)$/i)
      basename.sub(/\.([a-z]{2,3})(\.cc)?\.(srt|ass)$/i, ".#{language}\\2.\\3")
    else
      basename.sub(/\.(srt|ass)$/i, ".#{language}.\\1")
    end
  end

  def merged_text(original, translated, format: :srt)
    format == :ass ? "#{original}\\N#{translated}" : "#{original}\n#{translated}"
  end

  def completed_entries(part, ass:)
    return 0 unless File.file?(part)

    if ass
      File.foreach(part).count { |line| line.match?(/^Dialogue\s*:/i) }
    else
      File.read(part).split(/\n{2,}/).count { |block| !block.strip.empty? }
    end
  end

  def translate_file(file, output, source_lang, target_lang, jobs, merge: false)
    content = File.read(file)
    part = "#{output}.part"
    ass = File.extname(file).downcase == '.ass'
    lines = content.lines if ass
    entries = if ass
                lines.each_with_index.filter_map do |line, line_index|
                  parse_ass(line.chomp("\n"))&.merge(line_index: line_index)
                end
              else
                content.split(/\n{2,}/).map { |block| parse_srt(block) }
              end
    completed = completed_entries(part, ass: ass)
    raise "Checkpoint has more entries than the source subtitle file" if completed > entries.length

    output_file = File.open(part, completed.zero? ? 'w' : 'a')
    begin
      pending = entries.drop(completed)
      if ass
        cursor = completed.zero? ? 0 : entries[completed - 1][:line_index] + 1
        translate_entries(pending, jobs, source_lang, target_lang) do |index, text|
          entry = pending[index]
          output_file.write(lines[cursor...entry[:line_index]].join)
          source_line = lines[entry[:line_index]]
          newline = source_line.end_with?("\n") ? "\n" : ""
          text = merged_text(entry[:text], text, format: :ass) if merge
          output_file.write("#{entry[:prefix]} #{entry[:fields][0...9].join(',')},#{text}#{newline}")
          output_file.flush
          cursor = entry[:line_index] + 1
        end
        output_file.write(lines[cursor...].join)
      else
        translate_entries(pending, jobs, source_lang, target_lang) do |index, text|
          entry = pending[index]
          text = merged_text(entry[:text], text) if merge
          output_file.write([entry[:index], entry[:timestamp], text].join("\n"))
          output_file.write("\n\n") unless index == pending.length - 1
          output_file.flush
        end
        output_file.write("\n") unless content.end_with?("\n")
      end
      output_file.close
      FileUtils.mv(part, output)
    rescue Interrupt
      output_file.flush
      output_file.close
      warn "Interrupted; partial output saved to #{part}"
      raise
    rescue StandardError
      output_file.close unless output_file.closed?
      raise
    end
  end
end

if __FILE__ == $PROGRAM_NAME
  if ARGV.first == 'test'
    require 'minitest/autorun'
    class SubtitleTranslatorTest < Minitest::Test
      def test_parses_ass_dialogue_text
        entry = SubtitleTranslator.parse_ass('Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hello, world')
        assert_equal 'Hello, world', entry[:text]
        assert_equal 10, entry[:fields].length
      end

      def test_leaves_ass_non_dialogue_lines_alone
        assert_nil SubtitleTranslator.parse_ass('[Script Info]')
      end

      def test_merges_original_text_above_translation
        assert_equal "Original\nTranslation", SubtitleTranslator.merged_text('Original', 'Translation')
        assert_equal 'Original\\NTranslation', SubtitleTranslator.merged_text('Original', 'Translation', format: :ass)
      end

      def test_formats_retry_after_header
        assert_equal 'Google Translate rate limit (HTTP 429). Translation stopped. Retry-After: 60.',
                     SubtitleTranslator.rate_limit_message('60')
        assert_includes SubtitleTranslator.rate_limit_message(nil), 'No Retry-After header'
      end

      def test_uses_exponential_network_retry_delays
        assert_equal [1, 4, 9, 16], (1..4).map { |attempt| SubtitleTranslator.network_retry_delay(attempt) }
      end

      def test_uses_eight_workers_by_default_in_cli_definition
        source = File.read(__FILE__)
        assert_match(/default: 8/, source)
      end

      def test_counts_completed_part_entries
        Dir.mktmpdir do |dir|
          srt = File.join(dir, 'movie.srt.part')
          File.write(srt, "1\n00:00:00,000 --> 00:00:01,000\nOne\n\n2\n00:00:02,000 --> 00:00:03,000\nTwo\n")
          assert_equal 2, SubtitleTranslator.completed_entries(srt, ass: false)

          ass = File.join(dir, 'movie.ass.part')
          File.write(ass, "[Script Info]\nDialogue: 0,a,b,c,d,e,f,g,h\nDialogue: 0,a,b,c,d,e,f,g,i\n")
          assert_equal 2, SubtitleTranslator.completed_entries(ass, ass: true)
        end
      end
    end
  else
    require 'slop'
    begin
      options = Slop.parse do |o|
        o.string '-l', '--lang', 'Target language', default: 'vi'
        o.integer '-j', '--jobs', 'Number of threads', default: 8
        o.string '-o', '--output', 'Output file (optional)'
        o.bool '-m', '--merge', 'Show the original text above the translation'
        o.bool '-h', '--help', 'Show help'
      end
    rescue StandardError => e
      warn e.message
      exit 1
    end

    file = options.args.first
    if options[:help] || file.nil?
      puts options
      exit(file.nil? ? 1 : 0)
    end
    abort "File not found: #{file}" unless File.file?(file)
    abort 'Only .srt and .ass files are supported' unless %w[.srt .ass].include?(File.extname(file).downcase)

    target = options[:output] || SubtitleTranslator.output_path(file, options[:lang])
    abort "Aborting. file exists already '#{target}'" if File.exist?(target)
    part = "#{target}.part"
    if File.exist?(part)
      puts "Resuming checkpoint #{part}"
    else
      puts "Writing checkpoint to #{part}"
    end
    begin
      SubtitleTranslator.translate_file(file, target, SubtitleTranslator.source_language(file), options[:lang], options[:jobs], merge: options[:merge])
      puts "Translation complete; renamed #{part} to #{target}"
    rescue SubtitleTranslator::RateLimitError => error
      warn error.message
      exit 1
    end
  end
end
