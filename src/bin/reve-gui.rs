use iced::alignment;
use iced::widget::{button, column, container, text, text_input};
use iced::{Alignment, Application, Command, Element, Length, Renderer, Settings, Theme};

fn main() -> iced::Result {
    ReveGui::run(Settings::default())
}

#[derive(Debug, Default)]
struct ReveGui;

#[derive(Debug, Default)]
struct State {
    export_params: String,
    upscale_params: String,
    encode_params: String,
}

#[derive(Debug, Clone)]
enum Message {
    SelectInputPathPressed,
    SelectOutputPathPressed,
    ExportParamsChanged(String),
    UpscaleParamsChanged(String),
    EncodeParamsChanged(String),
    UpscalePressed,
}

impl Application for ReveGui {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        (Self::default(), Command::none())
    }

    fn title(&self) -> String {
        String::from("Reve-GUI")
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        Command::none()
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
        let export_params_input = text_input(
            "Ffmpeg export parameters",
            "",
            Message::ExportParamsChanged,
        );
        let upscale_params_input = text_input(
            "Real-ESRGAN upscale parameters",
            "",
            Message::UpscaleParamsChanged,
        );
        let encode_params_input = text_input(
            "Ffmpeg encode parameters",
            "",
            Message::EncodeParamsChanged,
        );
        let button = |label| {
            button(text(label).horizontal_alignment(alignment::Horizontal::Center))
                .padding(10)
                .width(Length::Units(80))
        };
        let upscale_button = button("Upscale");

        let content = column![
            export_params_input,
            upscale_params_input,
            encode_params_input,
            upscale_button,
        ]
        .width(Length::Fill)
        .align_items(Alignment::Center)
        .spacing(10);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .into()
    }
}
