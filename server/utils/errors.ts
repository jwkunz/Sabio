export class AppError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

export const toErrorMessage = (error: unknown) =>
  error instanceof Error ? error.message : "Unknown error";
